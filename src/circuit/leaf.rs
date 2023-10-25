use std::sync::{Arc, Mutex};

use super::{
    ipc::IpcChannel,
    memory::{self, Memory},
};
use crate::frequencies::FoCEstimator;

#[derive(Clone)]
pub struct Leaf {
    value: f64,
    frequency: f64,
    cluster: i32,
    foc_estimator: FoCEstimator,
    pub ipc_channel: Option<IpcChannel>,
    pub name: String,
    memory: Vec<Arc<Mutex<Memory>>>,
}

pub type Foliage = Arc<Mutex<Vec<Leaf>>>;

impl Leaf {
    pub fn new(value: &f64, frequency: &f64, name: &str) -> Self {
        Self {
            value: *value,
            frequency: *frequency,
            cluster: 0,
            foc_estimator: FoCEstimator::new(&frequency),
            ipc_channel: None,
            name: name.to_owned(),
            memory: vec![],
        }
    }

    pub fn get_value(&self) -> f64 {
        self.value
    }

    pub fn set_value(&mut self, value: &f64) {
        self.value = *value;
        self.frequency = self.foc_estimator.update();
    }

    pub fn set_cluster(&mut self, cluster: &i32) -> i32 {
        let cluster_step = self.cluster - *cluster;
        self.cluster = *cluster;
        cluster_step
    }

    pub fn get_cluster(&self) -> i32 {
        self.cluster
    }

    pub fn get_frequency(&self) -> f64 {
        self.frequency
    }

    pub fn add_dependency(&mut self, memory: Arc<Mutex<Memory>>) {
        self.memory.push(memory);
    }

    pub fn remove_dependency(&mut self, memory: Arc<Mutex<Memory>>) {
        self.memory.retain(|m| Arc::ptr_eq(m, &memory));
    }
}

pub fn update(foliage: Foliage, index: usize, value: &f64) {
    let mut foliage_guard = foliage.lock().unwrap();
    foliage_guard[index].set_value(value);
    for memory in &foliage_guard[index].memory {
        memory.lock().unwrap().invalidate();
    }
}

pub fn activate_channel(foliage: Foliage, index: usize, channel: &str, invert: &bool) {
    let channel = IpcChannel::new(
        foliage.clone(),
        index,
        channel.to_owned(),
        invert.to_owned(),
    )
    .unwrap();
    foliage.lock().unwrap()[index].ipc_channel = Some(channel);
}
