use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use crate::channels::FoCEstimator;
use crate::circuit::reactive::RcQueue;

use super::Vector;

#[derive(Clone)]
pub struct Leaf {
    value: Vector,
    frequency: f64,
    cluster: i32,
    foc_estimator: FoCEstimator,
    pub name: String,
    pub indices: BTreeSet<usize>,
}

pub type Foliage = Arc<Mutex<Vec<Leaf>>>;

impl Leaf {
    pub fn new(value: Vector, frequency: f64, name: &str) -> Self {
        Self {
            value: value.clone(),
            frequency,
            cluster: 0,
            foc_estimator: FoCEstimator::new(frequency),
            name: name.to_owned(),
            indices: BTreeSet::new(),
        }
    }

    pub fn get_value(&self) -> Vector {
        self.value.clone()
    }

    pub fn prune_frequency(&mut self, timestamp: f64, threshold: f64) {
        if timestamp - self.foc_estimator.timestamp.unwrap_or_default() >= threshold {
            self.foc_estimator.reset();
            self.frequency = 0.0;
        }
    }

    pub fn set_value(&mut self, value: Vector, timestamp: f64) -> bool {
        let difference = &value - &self.value;

        // Check if any difference in the value vector is larger than threshold
        // TODO: Make threshold leaf parameter or argument
        if difference.fold(false, |acc, value| acc | (value.abs() > 1e-3)) {
            self.value = value.clone();
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

pub fn update(foliage: &Foliage, rc_queue: &RcQueue, index: u16, value: Vector, timestamp: f64) {
    let mut foliage_guard = foliage.lock().unwrap();
    let mut queue_guard = rc_queue.lock().unwrap();

    if foliage_guard[index as usize].set_value(value, timestamp) {
        for rc_index in &foliage_guard[index as usize].indices {
            queue_guard.insert(*rc_index);
        }
    }
}
