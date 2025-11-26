use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::circuit::leaf::{update, Foliage};
use crate::circuit::reactive::RcQueue;

#[derive(Clone)]
pub struct IpcReader {
    pub topic: String,
    _handle: Arc<JoinHandle<()>>, // Keep handle to keep thread alive
}

pub struct IpcWriter {
    sender: Sender<(f64, f64)>,
}

pub struct TimedIpcWriter {
    pub frequency: f64,
    value: Arc<Mutex<f64>>,
    sender: Option<Sender<()>>,
    handle: Option<JoinHandle<()>>,
    writer: IpcWriter,
}

impl IpcReader {
    pub fn new(
        foliage: Foliage,
        rc_queue: RcQueue,
        index: u16,
        channel: &str,
        invert: bool,
        receiver: mpsc::Receiver<(f64, f64)>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let handle = std::thread::spawn(move || {
            while let Ok((value, timestamp)) = receiver.recv() {
                let final_value = if invert { 1.0 - value } else { value };
                update(&foliage, &rc_queue, index, final_value, timestamp);
            }
        });

        Ok(Self {
            topic: channel.to_owned(),
            _handle: Arc::new(handle),
        })
    }
}

impl IpcWriter {
    pub fn new(sender: Sender<(f64, f64)>) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self { sender })
    }

    pub fn write(&self, value: f64, timestamp: Option<f64>) {
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
        sender: Sender<(f64, f64)>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let value = Arc::new(Mutex::new(0.0));
        let writer = IpcWriter::new(sender)?;

        Ok(Self {
            frequency,
            value,
            sender: None,
            handle: None,
            writer,
        })
    }

    pub fn get_value_access(&self) -> Arc<Mutex<f64>> {
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
            let value = *thread_value.lock().unwrap();
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Acquiring timestamp failed!")
                .as_secs_f64();
            let _ = thread_writer.send((value, timestamp));

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
    use crate::circuit::leaf::Leaf;
    use std::collections::BTreeSet;
    use std::thread::sleep;

    #[test]
    fn test_ipc_read_write() -> Result<(), Box<dyn std::error::Error>> {
        let foliage = Arc::new(Mutex::new(vec![Leaf::new(0.0, 0.0, "test_leaf")]));
        let rc_queue = Arc::new(Mutex::new(BTreeSet::new()));
        let (tx, rx) = mpsc::channel();

        // Create reader
        let _reader = IpcReader::new(foliage.clone(), rc_queue.clone(), 0, "test_channel", false, rx)?;

        // Create writer
        let writer = IpcWriter::new(tx)?;

        // Initial value
        assert_eq!(foliage.lock().unwrap()[0].get_value(), 0.0);

        // Write a value
        writer.write(0.5, None);

        // Give the reader thread time to process
        sleep(Duration::from_millis(10));

        // Check updated value
        assert_eq!(foliage.lock().unwrap()[0].get_value(), 0.5);

        // Test inversion
        let (tx_invert, rx_invert) = mpsc::channel();
        let _reader_invert = IpcReader::new(foliage.clone(), rc_queue.clone(), 0, "test_channel_invert", true, rx_invert)?;
        let writer_invert = IpcWriter::new(tx_invert)?;

        writer_invert.write(0.8, None);
        sleep(Duration::from_millis(10));

        // The value should be 1.0 - 0.8
        assert!((foliage.lock().unwrap()[0].get_value() - 0.2).abs() < 1e-9);

        Ok(())
    }

    #[test]
    fn test_timed_ipc_writer() -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::channel();
        let mut timed_writer = TimedIpcWriter::new(100.0, tx)?; // 100 Hz

        // Get access to the value
        let value_access = timed_writer.get_value_access();
        *value_access.lock().unwrap() = 0.25;

        // Start the writer
        timed_writer.start();

        // Wait for a couple of cycles
        sleep(Duration::from_millis(25));

        // Stop the writer
        timed_writer.stop();

        // Change the value again
        *value_access.lock().unwrap() = 0.75;

        // Wait again
        sleep(Duration::from_millis(25));

        // Collect received values
        let mut received_values = vec![];
        while let Ok((val, _)) = rx.try_recv() {
            received_values.push(val);
        }

        // We should have received some values (likely 2 or 3)
        assert!(!received_values.is_empty());

        // All received values should be 0.25, as the writer was stopped before 0.75 was set
        for val in &received_values {
            assert_eq!(*val, 0.25);
        }

        // Check that no 0.75 values were sent
        assert!(!received_values.contains(&0.75));

        // Test drop behavior
        let (tx2, rx2) = mpsc::channel();
        {
            let mut timed_writer2 = TimedIpcWriter::new(100.0, tx2)?;
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
