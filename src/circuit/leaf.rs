use std::collections::BTreeSet;

use crate::channels::FoCEstimator;

use super::reactive::ReactiveCircuit;
use super::Vector;

#[derive(Clone, Debug)]
pub struct Leaf {
    value: Vector,
    frequency: f64,
    cluster: i32,
    foc_estimator: FoCEstimator,
    pub name: String,
    pub dependencies: BTreeSet<u32>,
}

impl Leaf {
    pub fn new(value: Vector, frequency: f64, name: &str) -> Self {
        Self {
            value: value.clone(),
            frequency,
            cluster: 0,
            foc_estimator: FoCEstimator::new(frequency),
            name: name.to_owned(),
            dependencies: BTreeSet::new(),
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

    pub fn get_dependencies(&self) -> BTreeSet<u32> {
        self.dependencies.clone()
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

    pub fn add_dependency(&mut self, index: u32) {
        self.dependencies.insert(index);
    }

    pub fn add_dependencies(&mut self, indices: &[u32]) {
        for index in indices {
            self.dependencies.insert(*index);
        }
    }

    pub fn clear_dependencies(&mut self) {
        self.dependencies.clear();
    }

    pub fn remove_dependency(&mut self, index: u32) {
        self.dependencies.remove(&index);
    }

    pub fn force_invalidate_dependencies(&mut self) {
        self.dependencies.clear();
    }
}

pub fn update(
    reactive_circuit: &mut ReactiveCircuit,
    leaf_index: u32,
    value: Vector,
    timestamp: f64,
) {
    let leaf = &mut reactive_circuit.leafs[leaf_index as usize];
    if leaf.set_value(value, timestamp) {
        for algebraic_circuit_index in &leaf.dependencies {
            reactive_circuit.queue.insert(*algebraic_circuit_index);
        }
    }
}

pub fn force_invalidate_dependencies(
    reactive_circuit: &mut ReactiveCircuit,
    leaf_index: u32,
) {
    let leaf = &mut reactive_circuit.leafs[leaf_index as usize];
    for algebraic_circuit_index in &leaf.dependencies {
        reactive_circuit.queue.insert(*algebraic_circuit_index);
    }
}