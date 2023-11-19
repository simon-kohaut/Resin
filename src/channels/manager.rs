use std::time::Duration;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};
use lazy_static::lazy_static;


use super::ipc::{IpcReader, IpcWriter};
use crate::circuit::{
    leaf::{Foliage, Leaf},
    reactive::RcQueue,
};

use rclrs::{spin, spin_once, Context, Node, RclrsError};

// We need this context to live throughout the programs lifetime
// Otherwise the ROS2 to Rust cleanup makes trouble
// All channel instantiations should be handled by Manager object
lazy_static! {
    static ref CONTEXT: Context = Context::new(vec![]).unwrap();
    static ref NODE: Mutex<Node> = Mutex::new(Node::new(&CONTEXT, "resin_ipc").unwrap());
}

pub struct Manager {
    pub foliage: Foliage,
    pub rc_queue: RcQueue,
    readers: Vec<IpcReader>,
    writers: Vec<IpcWriter>,
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

    pub fn write(&mut self, value: fn(f64) -> f64, channel: &str, frequency: f64) -> Result<(), RclrsError> {
        let mut writer = IpcWriter::new(&NODE.lock().unwrap(), channel, frequency, value)?;

        writer.start();
        self.writers.push(writer);
        Ok(())
    }

    pub fn stop_writers(&mut self) {
        self.writers.clear();
    }

    pub fn get_frequencies(&self) -> Vec<f64> {
        let foliage_guard = self.foliage.lock().unwrap();
        
        foliage_guard.iter().map(|leaf| leaf.get_frequency()).collect()
    }
    
    pub fn get_values(&self) -> Vec<f64> {
        let foliage_guard = self.foliage.lock().unwrap();
        
        foliage_guard.iter().map(|leaf| leaf.get_value()).collect()
    }
}


impl Drop for Manager {
    fn drop(&mut self) {
        self.stop_writers();
    }
}


#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_read_write() -> Result<(), RclrsError> {
        let mut manager = Manager::new();

        // Create a leaf and connect it with a reader and writer
        let receiver = manager.create_leaf("tester", 0.0, 0.0);
        manager.read(receiver, "/test", false)?;
        manager.write(|_| 1.0, "/test", 1.0)?;

        // Wait for a second
        use std::time::Duration;
        use std::thread::sleep;
        sleep(Duration::new(1, 0));

        // Before spinning, value should still be 0.0
        assert_eq!(manager.get_values(), vec![0.0]);

        // Leaf should now have value 1.0
        manager.spin_once();
        assert_eq!(manager.get_values(), vec![1.0]);

        manager.stop_writers();

        Ok(())
    }

    #[test]
    fn test_context_management() -> Result<(), RclrsError> {
        let mut manager = Manager::new();

        // Create a leaf and connect it with a reader and writer
        let receiver = manager.create_leaf("tester", 0.0, 0.0);
        manager.read(receiver, "/test", false)?;
        manager.write(|_| 1.0, "/test", 1.0)?;

        // Node should have 1 subscriber and 1 publisher
        assert_eq!(NODE.lock().unwrap().count_subscriptions("/test").unwrap(), 1);
        assert_eq!(NODE.lock().unwrap().count_publishers("/test").unwrap(), 1);

        drop(manager);

        // Everything should have stopped
        assert_eq!(NODE.lock().unwrap().count_subscriptions("/test").unwrap(), 0);
        assert_eq!(NODE.lock().unwrap().count_publishers("/test").unwrap(), 0);

        Ok(())
    }
}
