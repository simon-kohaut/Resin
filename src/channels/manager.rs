use std::time::Duration;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use super::ipc::{IpcReader, IpcWriter};
use crate::circuit::{
    leaf::{Foliage, Leaf},
    reactive::RcQueue,
};

use rclrs::{spin, spin_once, Context, Node, RclrsError};

pub struct Manager {
    context: Context,
    node: Node,
    pub foliage: Foliage,
    pub rc_queue: RcQueue,
    readers: Vec<IpcReader>,
    writers: Vec<IpcWriter>,
}

impl Manager {
    pub fn new() -> Self {
        let context = Context::new(vec![]).unwrap();
        let node = Node::new(&context, "resin_ipc").unwrap();

        Self {
            context,
            node,
            foliage: Arc::new(Mutex::new(vec![])),
            rc_queue: Arc::new(Mutex::new(BTreeSet::new())),
            readers: vec![],
            writers: vec![],
        }
    }

    pub fn create_leaf(&mut self, name: &str, value: f64, frequency: f64) -> u16 {
        self.foliage
            .lock()
            .unwrap()
            .push(Leaf::new(value, frequency, name));
        self.foliage.lock().unwrap().len() as u16 - 1
    }

    pub fn spin(self) {
        std::thread::spawn(move || {
            let _ = spin(&self.node);
        });
    }

    pub fn spin_once(&self) {
        let _ = spin_once(&self.node, Some(Duration::from_millis(0)));
    }

    pub fn read(&mut self, receiver: u16, channel: &str, invert: bool) -> Result<(), RclrsError> {
        let reader = IpcReader::new(
            &mut self.node,
            self.foliage.clone(),
            self.rc_queue.clone(),
            receiver,
            channel,
            invert,
        )?;

        self.readers.push(reader);
        Ok(())
    }

    pub fn write(&mut self, value: f64, channel: &str, frequency: f64) -> Result<(), RclrsError> {
        let writer = IpcWriter::new(&self.node, channel, frequency, value)?;

        writer.start();
        self.writers.push(writer);
        Ok(())
    }
}
