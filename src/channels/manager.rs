use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use super::ipc::{IpcReader, IpcWriter, TimedIpcWriter};
use super::Vector;
use crate::circuit::{leaf::Leaf, reactive::ReactiveCircuit};

use rclrs::{spin, spin_once, Context, Node, RclrsError};

// We need this context to live throughout the programs lifetime
// Otherwise the ROS2 to Rust cleanup makes trouble (segmentation fault, trying to drop context with active node, ...)
// All channel instantiations should be handled by Manager object
// use lazy_static::lazy_static;
// lazy_static! {
//     static ref CONTEXT: Context = Context::new(vec![]).unwrap();
//     static ref NODE: Mutex<Arc<Node>> = Mutex::new(Node::new(&CONTEXT, "resin_ipc").unwrap());
// }

pub struct Manager {
    pub reactive_circuit: Arc<Mutex<ReactiveCircuit>>,
    readers: Vec<IpcReader>,
    writers: Vec<TimedIpcWriter>,
    node: Arc<Node>,
}

impl Manager {
    pub fn new(value_size: usize) -> Self {
        Self {
            reactive_circuit: Arc::new(Mutex::new(ReactiveCircuit::new(value_size))),
            readers: vec![],
            writers: vec![],
            node: Node::new(&Context::new(vec![]).unwrap(), "resin_ipc").unwrap(),
        }
    }

    pub fn create_leaf(&mut self, name: &str, value: Vector, frequency: f64) -> u32 {
        // This should never grow beyong u32.MAX since we use that range for indexing
        assert!(self.reactive_circuit.lock().unwrap().leafs.len() + 1 < u32::MAX as usize);

        // Create a new leaf with given parameters and return the index
        self.reactive_circuit
            .lock()
            .unwrap()
            .leafs
            .push(Leaf::new(value, frequency, name));
        self.reactive_circuit.lock().unwrap().leafs.len() as u32 - 1
    }

    pub fn clear_dependencies(&mut self) {
        for leaf in self.reactive_circuit.lock().unwrap().leafs.iter_mut() {
            leaf.clear_dependencies();
        }

        self.reactive_circuit.lock().unwrap().queue.clear();
    }

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
            channel,
            invert,
        )?;

        self.readers.push(reader);
        Ok(())
    }

    pub fn make_writer(&mut self, channel: &str) -> Result<IpcWriter, RclrsError> {
        IpcWriter::new(self.node.clone(), channel)
    }

    pub fn make_timed_writer(
        &mut self,
        channel: &str,
        frequency: f64,
    ) -> Result<Arc<Mutex<f64>>, RclrsError> {
        let mut writer = TimedIpcWriter::new(self.node.clone(), channel, frequency)?;
        let value = writer.get_value_access();

        writer.start();
        self.writers.push(writer);

        Ok(value)
    }

    pub fn stop_timed_writers(&mut self) {
        self.writers.clear();
    }

    pub fn prune_frequencies(&self, threshold: f64, timestamp: Option<f64>) {
        let mut reactive_circuit_guard = self.reactive_circuit.lock().unwrap();

        let timestamp = if timestamp.is_none() {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Acquiring UNIX timestamp failed!")
                .as_secs_f64()
        } else {
            timestamp.unwrap()
        };

        for leaf in &mut reactive_circuit_guard.leafs.iter_mut() {
            leaf.prune_frequency(timestamp, threshold);
        }
    }

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

    pub fn get_names(&self) -> Vec<String> {
        let reactive_circuit_guard = self.reactive_circuit.lock().unwrap();

        reactive_circuit_guard
            .leafs
            .iter()
            .map(|leaf| leaf.name.to_owned())
            .collect()
    }

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

    #[test]
    fn test_read_write() -> Result<(), RclrsError> {
        let mut manager = Manager::new(1);

        // Create a leaf and connect it with a reader and writer
        let receiver = manager.create_leaf("tester_1", Vector::from(vec![0.0]), 0.0);
        manager.read(receiver, "/test_1", false)?;
        let value = manager.make_timed_writer("/test_1", 1.0)?;
        *value.lock().unwrap() = 1.0;

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
}
