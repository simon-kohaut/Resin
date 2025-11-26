use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use crate::channels::FoCEstimator;
use crate::circuit::reactive::RcQueue;

#[derive(Clone)]
pub struct Leaf {
    value: f64,
    frequency: f64,
    cluster: i32,
    foc_estimator: FoCEstimator,
    pub name: String,
    pub indices: BTreeSet<usize>,
}

pub type Foliage = Arc<Mutex<Vec<Leaf>>>;

impl Leaf {
    pub fn new(value: f64, frequency: f64, name: &str) -> Self {
        Self {
            value,
            frequency,
            cluster: 0,
            foc_estimator: FoCEstimator::new(frequency),
            name: name.to_owned(),
            indices: BTreeSet::new(),
        }
    }

    pub fn get_value(&self) -> f64 {
        self.value
    }

    pub fn prune_frequency(&mut self, timestamp: f64, threshold: f64) {
        if timestamp - self.foc_estimator.timestamp.unwrap_or_default() >= threshold {
            self.foc_estimator = FoCEstimator::new(0.0);
            self.frequency = 0.0
        }
    }

    pub fn set_value(&mut self, value: f64, timestamp: f64) -> bool {
        if (value - self.value).abs() > 1e-3 {
            self.value = value;
            self.frequency = self.foc_estimator.update(timestamp);

            true
        } else {
            false
        }
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

    pub fn set_frequency(&mut self, frequency: &f64) {
        self.frequency = *frequency;
    }

    pub fn add_dependency(&mut self, index: usize) {
        self.indices.insert(index);
    }

    pub fn add_dependencies(&mut self, indices: &[usize]) {
        for index in indices {
            self.indices.insert(*index);
        }
    }

    pub fn clear_dependencies(&mut self) {
        self.indices.clear();
    }

    pub fn remove_dependency(&mut self, index: usize) {
        self.indices.remove(&index);
    }
}

pub fn update(foliage: &Foliage, rc_queue: &RcQueue, index: u16, value: f64, timestamp: f64) {
    let mut foliage_guard = foliage.lock().unwrap();
    let mut queue_guard = rc_queue.lock().unwrap();

    if foliage_guard[index as usize].set_value(value, timestamp) {
        for rc_index in &foliage_guard[index as usize].indices {
            queue_guard.insert(*rc_index);
        }
    }
}
