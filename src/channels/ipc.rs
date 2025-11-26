use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::circuit::leaf::update;
use crate::circuit::ReactiveCircuit;

use super::Vector;

#[derive(Clone)]
pub struct IpcReader {
    pub topic: String,
    _handle: Arc<JoinHandle<()>>, // Keep handle to keep thread alive
}

pub struct IpcWriter {
    sender: Sender<(Vector, f64)>,
}

pub struct TimedIpcWriter {
    pub frequency: f64,
    value: Arc<Mutex<Vector>>,
    sender: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
    writer: IpcWriter,
}

impl IpcReader {
    pub fn new(
        shared_reactive_circuit: Arc<Mutex<ReactiveCircuit>>,
        index: u32,
        channel: &str,
        invert: bool,
        receiver: mpsc::Receiver<(Vector, f64)>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let handle = std::thread::spawn(move || {
            while let Ok((value, timestamp)) = receiver.recv() {
                let final_value = if invert { 1.0 - value } else { value };
                update(&mut shared_reactive_circuit.lock().unwrap(), index, final_value, timestamp);
            }
        });

        Ok(Self {
            topic: channel.to_owned(),
            _handle: Arc::new(handle),
        })
    }
}

impl IpcWriter {
    pub fn new(sender: Sender<(Vector, f64)>) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { sender })
    }

    pub fn write(&self, value: Vector, timestamp: Option<f64>) {
        let timestamp = if timestamp.is_none() {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Acquiring UNIX timestamp failed!")
                .as_secs_f64()
        } else {
            timestamp.unwrap()
        };

        let _ = self.sender.send((value, timestamp));
    }
}

impl TimedIpcWriter {
    pub fn new(
        frequency: f64,
        sender: Sender<(Vector, f64)>,
        value: Vector
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let writer = IpcWriter::new(sender)?;

        Ok(Self {
            frequency,
            value: Arc::new(Mutex::new(value)),
            sender: None,
            handle: None,
            writer,
        })
    }

    pub fn get_value_access(&self) -> Arc<Mutex<Vector>> {
        self.value.clone()
    }

    pub fn start(&mut self) {
        use std::thread::spawn;

        // If this is already running, we don't do anything
        if self.sender.is_some() {
            return;
        }

        // Make copies such that self isn't moved here
        let thread_value = self.value.clone();
        let thread_timeout = Duration::from_secs_f64(1.0 / self.frequency);
        let thread_writer = self.writer.sender.clone();

        // Create a channel to later terminate the thread
        let (sender, receiver) = mpsc::channel();
        self.sender = Some(sender);

        self.handle = Some(spawn(move || loop {
            let value = thread_value.lock().unwrap();
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Acquiring timestamp failed!")
                .as_secs_f64();
            let _ = thread_writer.send((value.clone(), timestamp));

            // Break if notified via channel or disconnected
            match receiver.recv_timeout(thread_timeout) {
                Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => (),
            }
        }));
    }

    pub fn stop(&mut self) {
        if self.sender.is_some() {
            if let Some(sender) = self.sender.take() {
                // The send might fail if the receiver is already gone, which is fine.
                let _ = sender.send(());
            }
            if let Some(handle) = self.handle.take() {
                handle
                    .join()
                    .expect("Could not join with writer thread!");
            }
        }
    }
}

impl Drop for TimedIpcWriter {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::array;
    use std::thread::sleep;

    #[test]
    fn test_ipc_read_write() -> Result<(), Box<dyn std::error::Error>> {
        let reactive_circuit = Arc::new(Mutex::new(ReactiveCircuit::new(1)));
        reactive_circuit
            .lock()
            .unwrap()
            .leafs
            .push(crate::circuit::leaf::Leaf::new(
                array![0.0].into(),
                0.0,
                "test_leaf",
            ));
        let (tx, rx) = mpsc::channel();

        // Create reader
        let _reader = IpcReader::new(reactive_circuit.clone(), 0, "test_channel", false, rx)?;

        // Create writer
        let writer = IpcWriter::new(tx)?;

        // Initial value
        assert_eq!(
            reactive_circuit.lock().unwrap().leafs[0].get_value(),
            array![0.0]
        );

        // Write a value
        writer.write(array![0.5].into(), None);

        // Give the reader thread time to process
        sleep(Duration::from_millis(20));

        // Check updated value
        assert_eq!(
            reactive_circuit.lock().unwrap().leafs[0].get_value(),
            array![0.5]
        );

        // Test inversion
        let (tx_invert, rx_invert) = mpsc::channel();
        let _reader_invert = IpcReader::new(
            reactive_circuit.clone(),
            0,
            "test_channel_invert",
            true,
            rx_invert,
        )?;
        let writer_invert = IpcWriter::new(tx_invert)?;

        writer_invert.write(array![0.8].into(), None);
        sleep(Duration::from_millis(20));

        // The value should be 1.0 - 0.8
        assert!((reactive_circuit.lock().unwrap().leafs[0].get_value() - array![0.2]).sum().abs() < 1e-9);

        Ok(())
    }

    #[test]
    fn test_timed_ipc_writer() -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::channel();
        let mut timed_writer = TimedIpcWriter::new(100.0, tx, array![0.0].into())?; // 100 Hz

        // Get access to the value
        let value_access = timed_writer.get_value_access();
        *value_access.lock().unwrap() = array![0.25].into();

        // Start the writer
        timed_writer.start();

        // Wait for a couple of cycles
        sleep(Duration::from_millis(30));

        // Stop the writer
        timed_writer.stop();

        // Change the value again
        *value_access.lock().unwrap() = array![0.75].into();

        // Wait again
        sleep(Duration::from_millis(30));

        // Collect received values
        let mut received_values = vec![];
        while let Ok((val, _)) = rx.try_recv() {
            received_values.push(val);
        }

        // We should have received some values (likely 2 or 3)
        assert!(!received_values.is_empty());

        // All received values should be 0.25, as the writer was stopped before 0.75 was set
        for val in &received_values {
            assert_eq!(*val, array![0.25]);
        }

        // Check that no 0.75 values were sent
        assert!(!received_values.contains(&array![0.75].into()));

        // Test drop behavior
        let (tx2, rx2) = mpsc::channel();
        {
            let mut timed_writer2 = TimedIpcWriter::new(100.0, tx2, array![0.0].into())?;
            timed_writer2.start();
        } // timed_writer2 is dropped here, stopping the thread

        // Drain the channel for possible remaining data
        while rx2.try_recv().is_ok() {
            // Keep draining
        }

        // Now that the channel is empty, the next call should show it's disconnected
        assert_eq!(
            rx2.try_recv(),
            Err(mpsc::TryRecvError::Disconnected),
            "Channel should be disconnected after writer is dropped"
        );

        Ok(())
    }
}
