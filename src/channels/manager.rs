use lazy_static::lazy_static;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
use std::{
    collections::BTreeSet,
    collections::HashMap,
    sync::{Arc, Mutex},
};

use super::ipc::{IpcReader, IpcWriter, TimedIpcWriter};
use crate::circuit::{
    leaf::{Foliage, Leaf},
    reactive::RcQueue,
};

use rclrs::{spin, spin_once, Context, Node, RclrsError};

// We need this context to live throughout the programs lifetime
// Otherwise the ROS2 to Rust cleanup makes trouble (segmentation fault, trying to drop context with active node, ...)
// All channel instantiations should be handled by Manager object
lazy_static! {
    static ref CONTEXT: Context = Context::new(vec![]).unwrap();
    static ref NODE: Mutex<Node> = Mutex::new(Node::new(&CONTEXT, "resin_ipc").unwrap());
}

pub struct Manager {
    pub foliage: Foliage,
    pub rc_queue: RcQueue,
    readers: Vec<IpcReader>,
    writers: Vec<TimedIpcWriter>,
}

impl Manager {
    pub fn new() -> Self {
        Self {
            foliage: Arc::new(Mutex::new(vec![])),
            rc_queue: Arc::new(Mutex::new(BTreeSet::new())),
            readers: vec![],
            writers: vec![],
        }
    }

    pub fn create_leaf(&mut self, name: &str, value: f64, frequency: f64) -> u16 {
        // This should never grow beyong u16.MAX since we use that range for indexing
        assert!(self.foliage.lock().unwrap().len() + 1 < u16::MAX.into());

        // Create a new leaf with given parameters and return the index
        self.foliage
            .lock()
            .unwrap()
            .push(Leaf::new(value, frequency, name));
        self.foliage.lock().unwrap().len() as u16 - 1
    }

    pub fn clear_dependencies(&mut self) {
        for leaf in self.foliage.lock().unwrap().iter_mut() {
            leaf.clear_dependencies();
        }

        self.rc_queue.lock().unwrap().clear();
    }

    pub fn spin(self) {
        std::thread::spawn(move || {
            let _ = spin(&NODE.lock().unwrap());
        });
    }

    pub fn spin_once(&self) {
        let _ = spin_once(&NODE.lock().unwrap(), Some(Duration::from_millis(0)));
    }

    pub fn read(&mut self, receiver: u16, channel: &str, invert: bool) -> Result<(), RclrsError> {
        let reader = IpcReader::new(
            &mut NODE.lock().unwrap(),
            self.foliage.clone(),
            self.rc_queue.clone(),
            receiver,
            channel,
            invert,
        )?;

        self.readers.push(reader);
        Ok(())
    }

    pub fn make_writer(&mut self, channel: &str) -> Result<IpcWriter, RclrsError> {
        IpcWriter::new(&NODE.lock().unwrap(), channel)
    }

    pub fn make_timed_writer(&mut self, channel: &str, frequency: f64) -> Result<Arc<Mutex<f64>>, RclrsError> {
        let mut writer = TimedIpcWriter::new(&NODE.lock().unwrap(), channel, frequency)?;
        let value = writer.get_value_access();

        writer.start();
        self.writers.push(writer);

        Ok(value)
    }

    pub fn stop_timed_writers(&mut self) {
        self.writers.clear();
    }

    pub fn prune_frequencies(&self, threshold: f64, timestamp: Option<f64>) {
        let mut foliage_guard = self.foliage.lock().unwrap();

        let timestamp = if timestamp.is_none() {
            SystemTime::now().duration_since(UNIX_EPOCH).expect("Acquiring UNIX timestamp failed!").as_secs_f64()
        } else {
            timestamp.unwrap()
        };

        for leaf in &mut foliage_guard.iter_mut() {
            leaf.prune_frequency(timestamp, threshold);
        }
    }

    pub fn get_frequencies(&self) -> Vec<f64> {
        let foliage_guard = self.foliage.lock().unwrap();

        foliage_guard
            .iter()
            .map(|leaf| leaf.get_frequency())
            .collect()
    }

    pub fn get_values(&self) -> Vec<f64> {
        let foliage_guard = self.foliage.lock().unwrap();

        foliage_guard.iter().map(|leaf| leaf.get_value()).collect()
    }

    pub fn get_names(&self) -> Vec<String> {
        let foliage_guard = self.foliage.lock().unwrap();

        foliage_guard
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

    use super::*;

    #[test]
    fn test_read_write() -> Result<(), RclrsError> {
        let mut manager = Manager::new();

        // Create a leaf and connect it with a reader and writer
        let receiver = manager.create_leaf("tester_1", 0.0, 0.0);
        manager.read(receiver, "/test_1", false)?;
        let value = manager.make_timed_writer("/test_1", 1.0)?;
        *value.lock().unwrap() = 1.0;

        // Wait for long enough that we must have a result
        // The recv_timeout internally can be a bit slow so we add a millisecond
        use std::thread::sleep;
        use std::time::Duration;
        sleep(Duration::new(2, 0));

        // Before spinning, value should still be 0.0
        assert_eq!(manager.get_values(), vec![0.0]);

        // Leaf should now have value 1.0
        manager.spin_once();
        assert_eq!(manager.get_values(), vec![1.0]);

        Ok(())
    }

    #[test]
    fn test_context_management() -> Result<(), RclrsError> {
        let mut manager = Manager::new();

        // Create a leaf and connect it with a reader and writer
        let receiver = manager.create_leaf("tester_2", 0.0, 0.0);
        manager.read(receiver, "/test_2", false)?;
        let value = manager.make_timed_writer("/test_2", 1.0)?;
        *value.lock().unwrap() = 1.0;

        // Node should have 1 subscriber and 1 publisher
        assert_eq!(
            NODE.lock().unwrap().count_subscriptions("/test_2").unwrap(),
            1
        );
        assert_eq!(NODE.lock().unwrap().count_publishers("/test_2").unwrap(), 1);

        drop(manager);

        // Everything should have stopped
        assert_eq!(
            NODE.lock().unwrap().count_subscriptions("/test_2").unwrap(),
            0
        );
        assert_eq!(NODE.lock().unwrap().count_publishers("/test_2").unwrap(), 0);

        Ok(())
    }
}
