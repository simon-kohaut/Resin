<<<<<<< HEAD
=======
use std::time::{Duration, SystemTime, UNIX_EPOCH};
>>>>>>> origin/graph-based-rc
use std::{
    collections::HashMap,
    sync::mpsc,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use super::ipc::{IpcReader, IpcWriter, TimedIpcWriter};
use super::Vector;
use crate::circuit::{leaf::Leaf, reactive::ReactiveCircuit};

<<<<<<< HEAD
/// Manages the state of leaves (Foliage) and the IPC channels for updating them.
///
/// The `Manager` is a central struct that holds the collection of `Leaf` nodes,
/// a queue for reactive circuits that need updates (`rc_queue`), and the associated
/// readers and writers for inter-process communication. It handles the creation of
/// leaves and the setup of channels to read from or write to, including timed writers
/// that send data at a specified frequency.
=======
use rclrs::{spin, spin_once, Context, Node, RclrsError};

// We need this context to live throughout the programs lifetime
// Otherwise the ROS2 to Rust cleanup makes trouble (segmentation fault, trying to drop context with active node, ...)
// All channel instantiations should be handled by Manager object
// use lazy_static::lazy_static;
// lazy_static! {
//     static ref CONTEXT: Context = Context::new(vec![]).unwrap();
//     static ref NODE: Mutex<Arc<Node>> = Mutex::new(Node::new(&CONTEXT, "resin_ipc").unwrap());
// }

>>>>>>> origin/graph-based-rc
pub struct Manager {
    pub reactive_circuit: Arc<Mutex<ReactiveCircuit>>,
    readers: Vec<IpcReader>,
    writers: Vec<TimedIpcWriter>,
<<<<<<< HEAD
    senders: HashMap<String, mpsc::Sender<(f64, f64)>>,
}

impl Default for Manager {
    fn default() -> Self {
        Self::new()
    }
=======
    node: Arc<Node>,
>>>>>>> origin/graph-based-rc
}

impl Manager {
    pub fn new(value_size: usize) -> Self {
        Self {
            reactive_circuit: Arc::new(Mutex::new(ReactiveCircuit::new(value_size))),
            readers: vec![],
            writers: vec![],
<<<<<<< HEAD
            senders: HashMap::new(),
        }
    }

    /// Creates a new `Leaf` and adds it to the foliage.
    ///
    /// # Returns
    /// The index of the newly created leaf as a `u16`.
    pub fn create_leaf(&mut self, name: &str, value: f64, frequency: f64) -> u16 {
        // This should never grow beyong u16.MAX since we use that range for indexing
        assert!(self.foliage.lock().unwrap().len() + 1 < u16::MAX.into());
=======
            node: Node::new(&Context::new(vec![]).unwrap(), "resin_ipc").unwrap(),
        }
    }

    pub fn create_leaf(&mut self, name: &str, value: Vector, frequency: f64) -> u32 {
        // This should never grow beyong u32.MAX since we use that range for indexing
        assert!(self.reactive_circuit.lock().unwrap().leafs.len() + 1 < u32::MAX as usize);
>>>>>>> origin/graph-based-rc

        // Create a new leaf with given parameters and return the index
        self.reactive_circuit
            .lock()
            .unwrap()
            .leafs
            .push(Leaf::new(value, frequency, name));
        self.reactive_circuit.lock().unwrap().leafs.len() as u32 - 1
    }

    /// Clears all dependency indices from all leaves and clears the reactive queue.
    pub fn clear_dependencies(&mut self) {
        for leaf in self.reactive_circuit.lock().unwrap().leafs.iter_mut() {
            leaf.clear_dependencies();
        }

        self.reactive_circuit.lock().unwrap().queue.clear();
    }

<<<<<<< HEAD
    /// Creates a reader for a given channel that updates a leaf.
    ///
    /// # Arguments
    /// * `receiver_idx` - The index of the leaf to be updated by this reader.
    /// * `channel` - The name of the IPC channel.
    /// * `invert` - If true, the received value will be inverted (1.0 - value).
    pub fn read(
        &mut self,
        receiver_idx: u16,
        channel: &str,
        invert: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::channel();
        self.senders.insert(channel.to_string(), tx);
        let reader = IpcReader::new(
            self.foliage.clone(),
            self.rc_queue.clone(),
            receiver_idx,
=======
    pub fn spin(self) {
        std::thread::spawn(move || {
            let _ = spin(self.node.clone());
        });
    }

    pub fn spin_once(&self) {
        let _ = spin_once(self.node.clone(), Some(Duration::from_millis(0)));
    }

    pub fn read(&mut self, receiver: u32, channel: &str, invert: bool) -> Result<(), RclrsError> {
        let reader = IpcReader::new(
            self.node.clone(),
            self.reactive_circuit.clone(),
            receiver,
>>>>>>> origin/graph-based-rc
            channel,
            invert,
            rx,
        )?;

        self.readers.push(reader);
        Ok(())
    }

<<<<<<< HEAD
    /// Creates a writer for a given channel.
    pub fn make_writer(&mut self, channel: &str) -> Result<IpcWriter, Box<dyn std::error::Error>> {
        if let Some(sender) = self.senders.get(channel) {
            IpcWriter::new(sender.clone())
        } else {
            let (tx, _rx) = mpsc::channel();
            self.senders.insert(channel.to_string(), tx);
            // This reader will be dropped if nothing reads from it, closing the channel.
            // This is a simplification. In a real scenario you might want to handle this differently.
            IpcWriter::new(self.senders.get(channel).unwrap().clone())
        }
    }

    /// Creates a timed writer that sends its value at a given frequency.
=======
    pub fn make_writer(&mut self, channel: &str) -> Result<IpcWriter, RclrsError> {
        IpcWriter::new(self.node.clone(), channel)
    }

>>>>>>> origin/graph-based-rc
    pub fn make_timed_writer(
        &mut self,
        channel: &str,
        frequency: f64,
<<<<<<< HEAD
    ) -> Result<Arc<Mutex<f64>>, Box<dyn std::error::Error>> {
        let writer_tx = self
            .senders
            .entry(channel.to_string())
            .or_insert_with(|| mpsc::channel().0)
            .clone();
        let mut writer = TimedIpcWriter::new(frequency, writer_tx)?;
=======
    ) -> Result<Arc<Mutex<f64>>, RclrsError> {
        let mut writer = TimedIpcWriter::new(self.node.clone(), channel, frequency)?;
>>>>>>> origin/graph-based-rc
        let value = writer.get_value_access();

        writer.start();
        self.writers.push(writer);

        Ok(value)
    }

    /// Stops and removes all active timed writers.
    pub fn stop_timed_writers(&mut self) {
        self.writers.clear();
    }

    /// Prunes the frequencies of all leaves based on a timestamp threshold.
    pub fn prune_frequencies(&self, threshold: f64, timestamp: Option<f64>) {
        let mut reactive_circuit_guard = self.reactive_circuit.lock().unwrap();

        let timestamp = if let Some(ts) = timestamp {
            ts
        } else {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Acquiring UNIX timestamp failed!")
                .as_secs_f64()
        };

        for leaf in &mut reactive_circuit_guard.leafs.iter_mut() {
            leaf.prune_frequency(timestamp, threshold);
        }
    }

    /// Returns a vector of the frequencies of all leaves.
    pub fn get_frequencies(&self) -> Vec<f64> {
        let reactive_circuit_guard = self.reactive_circuit.lock().unwrap();

        reactive_circuit_guard
            .leafs
            .iter()
            .map(|leaf| leaf.get_frequency())
            .collect()
    }

    pub fn get_values(&self) -> Vec<Vector> {
        let reactive_circuit_guard = self.reactive_circuit.lock().unwrap();

        reactive_circuit_guard
            .leafs
            .iter()
            .map(|leaf| leaf.get_value().clone())
            .collect()
    }

    /// Returns a vector of the names of all leaves.
    pub fn get_names(&self) -> Vec<String> {
        let reactive_circuit_guard = self.reactive_circuit.lock().unwrap();

        reactive_circuit_guard
            .leafs
            .iter()
            .map(|leaf| leaf.name.to_owned())
            .collect()
    }

    /// Returns a `HashMap` mapping leaf names to their indices.
    pub fn get_index_map(&self) -> HashMap<String, usize> {
        let names = self.get_names();
        let mut map = HashMap::new();

        for name in &names {
            let position = names
                .iter()
                .position(|leaf_name| *leaf_name == *name)
                .expect("Error during creation of index map!");
            map.insert(name.to_owned(), position);
        }

        map
    }
}

impl Drop for Manager {
    fn drop(&mut self) {
        self.stop_timed_writers();
    }
}

#[cfg(test)]
mod tests {

    use ndarray::array;

    use super::*;
    use std::{thread::sleep, time::Duration};

    #[test]
    fn test_read_write() -> Result<(), Box<dyn std::error::Error>> {
        let mut manager = Manager::new(1);

        // Create a leaf and connect it with a reader and writer
        let receiver = manager.create_leaf("tester_1", Vector::from(vec![0.0]), 0.0);
        manager.read(receiver, "/test_1", false)?;
        let writer = manager.make_writer("/test_1")?;

        // Wait for long enough that we must have a result
        // The recv_timeout internally can be a bit slow so we add a millisecond
        use std::thread::sleep;
        use std::time::Duration;
        sleep(Duration::new(2, 0));

        // Before spinning, value should still be 0.0
        assert_eq!(manager.get_values(), vec![array![0.0]]);

        // Leaf should now have value 1.0
        manager.spin_once();
        assert_eq!(manager.get_values(), vec![array![1.0]]);

        Ok(())
    }

    #[test]
    fn test_timed_writer() -> Result<(), Box<dyn std::error::Error>> {
        let mut manager = Manager::new();
        let receiver = manager.create_leaf("timed_tester", 0.0, 0.0);
        manager.read(receiver, "/timed_test", false)?;

        // Create a timed writer with a frequency of 100 Hz (sends every 10ms)
        let value_access = manager.make_timed_writer("/timed_test", 100.0)?;

        // Initial value should be 0.0
        assert_eq!(manager.get_values(), vec![0.0]);

        // Update the value that the timed writer sends
        *value_access.lock().unwrap() = 0.75;

        // Wait for a few cycles to ensure the value is sent and received
        sleep(Duration::from_millis(30));

        // The leaf should be updated
        assert_eq!(manager.get_values(), vec![0.75]);

        // The writer is stopped when the manager is dropped.
        // We can also test explicit stop.
        manager.stop_timed_writers();

        // Update value again
        *value_access.lock().unwrap() = 0.25;

        // Wait and check that the value is NOT updated because the writer is stopped.
        sleep(Duration::from_millis(30));
        assert_eq!(manager.get_values(), vec![0.75]);
    }
    
    fn test_context_management() -> Result<(), RclrsError> {
        let mut manager = Manager::new(1);

        // Create a leaf and connect it with a reader and writer
        let receiver = manager.create_leaf("tester_2", Vector::from(vec![0.0]), 0.0);
        manager.read(receiver, "/test_2", false)?;
        let value = manager.make_timed_writer("/test_2", 1.0)?;
        *value.lock().unwrap() = 1.0;

        // Node should have 1 subscriber and 1 publisher
        assert_eq!(manager.node.count_subscriptions("/test_2").unwrap(), 1);
        assert_eq!(manager.node.count_publishers("/test_2").unwrap(), 1);

        Ok(())
    }

    #[test]
    fn test_multiple_channels() -> Result<(), Box<dyn std::error::Error>> {
        let mut manager = Manager::new();

        let r1 = manager.create_leaf("r1", 0.0, 0.0);
        let r2 = manager.create_leaf("r2", 0.0, 0.0);

        manager.read(r1, "/chan1", false)?;
        manager.read(r2, "/chan2", true)?; // This one inverts

        let w1 = manager.make_writer("/chan1")?;
        let w2 = manager.make_writer("/chan2")?;

        assert_eq!(manager.get_values(), vec![0.0, 0.0]);

        w1.write(0.5, None);
        w2.write(0.8, None);

        sleep(Duration::from_millis(10));

        assert_eq!(manager.get_values(), vec![0.5, 0.19999999999999996]); // 1.0 - 0.8

        Ok(())
    }

    #[test]
    fn test_prune_frequencies() {
        let mut manager = Manager::new();
        let leaf_idx = manager.create_leaf("freq_leaf", 0.5, 0.0);
        let mut leaf_guard = manager.foliage.lock().unwrap();
        let leaf = &mut leaf_guard[leaf_idx as usize];

        // Send multiple values at fixed frequence
        for i in 0..100 {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs_f64();
            leaf.set_value(1.0 / i as f64, now);
            sleep(Duration::from_millis(10));
        }
        drop(leaf_guard);

        // Frequency should now be about 
        assert!(manager.get_frequencies()[0] - 100.0 < 1e-3);

        // Prune with a threshold of 10s, should not prune
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        manager.prune_frequencies(10.0, Some(now));
        assert!(manager.get_frequencies()[0] - 100.0 < 1e-3);

        // Wait for 1s and prune
        sleep(Duration::from_millis(1000));
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        manager.prune_frequencies(1.0, Some(now));
        assert_eq!(manager.get_frequencies()[0], 0.0);
    }

    #[test]
    fn test_getters() {
        let mut manager = Manager::new();
        manager.create_leaf("a", 0.1, 1.0);
        manager.create_leaf("b", 0.2, 2.0);

        assert_eq!(manager.get_names(), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(manager.get_values(), vec![0.1, 0.2]);
        assert_eq!(manager.get_frequencies(), vec![1.0, 2.0]);

        let index_map = manager.get_index_map();
        assert_eq!(*index_map.get("a").unwrap(), 0);
        assert_eq!(*index_map.get("b").unwrap(), 1);
    }
}
