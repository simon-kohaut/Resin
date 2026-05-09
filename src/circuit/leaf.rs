use std::collections::BTreeSet;

use crate::channels::FoCEstimator;

use super::reactive::ReactiveCircuit;
use super::Vector;

/// A time-dynamic input node in the reactive circuit.
///
/// Each `Leaf` holds a `Vector` of current probability values (one per
/// value-space slot), a Frequency-of-Change (FoC) estimate, a cluster label
/// used for adaptive partitioning, and the set of `AlgebraicCircuit` node
/// indices that depend on this leaf's value.
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
    /// Creates a new leaf with the given initial `value`, FoC `frequency`, and `name`.
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

    /// Returns a clone of the current value vector.
    pub fn get_value(&self) -> Vector {
        self.value.clone()
    }

    /// Resets the FoC estimator and sets `frequency` to `0.0` if the leaf has
    /// not been updated within `threshold` seconds of `timestamp`.
    pub fn prune_frequency(&mut self, timestamp: f64, threshold: f64) {
        if timestamp - self.foc_estimator.timestamp.unwrap_or_default() >= threshold {
            self.foc_estimator.reset();
            self.frequency = 0.0;
        }
    }

    /// Updates the leaf value and FoC estimate if the new `value` differs from
    /// the current value by more than `1e-3` in any element.
    ///
    /// Returns `true` when the value was changed (dependent circuits should be
    /// re-queued), `false` when the change was below threshold (no-op).
    pub fn set_value(&mut self, value: Vector, timestamp: f64) -> bool {
        let difference = &value - &self.value;

        // Check if any difference in the value vector is larger than the threshold
        // TODO: Make threshold leaf parameter or argument
        if difference.iter().any(|&d| d.abs() > 1e-3) {
            self.value = value.clone();
            self.frequency = self.foc_estimator.update(timestamp);

            true
        } else {
            false
        }
    }

    /// Sets the cluster label and returns the signed step `old − new`, which
    /// indicates how many cluster levels this leaf moved (positive = dropped,
    /// negative = lifted).
    pub fn set_cluster(&mut self, cluster: &i32) -> i32 {
        let cluster_step = self.cluster - *cluster;
        self.cluster = *cluster;
        cluster_step
    }

    /// Returns a clone of the set of `AlgebraicCircuit` node indices that
    /// depend on this leaf.
    pub fn get_dependencies(&self) -> BTreeSet<u32> {
        self.dependencies.clone()
    }

    /// Returns the current cluster label.
    pub fn get_cluster(&self) -> i32 {
        self.cluster
    }

    /// Returns the current estimated Frequency-of-Change in Hz.
    pub fn get_frequency(&self) -> f64 {
        self.frequency
    }

    /// Overrides the FoC estimate directly (used during adaptive partitioning).
    pub fn set_frequency(&mut self, frequency: &f64) {
        self.frequency = *frequency;
    }

    /// Registers `index` as a dependent `AlgebraicCircuit` node.
    pub fn add_dependency(&mut self, index: u32) {
        self.dependencies.insert(index);
    }

    /// Registers multiple dependent node indices at once.
    pub fn add_dependencies(&mut self, indices: &[u32]) {
        for index in indices {
            self.dependencies.insert(*index);
        }
    }

    /// Removes all dependency registrations (call before rebuilding the circuit).
    pub fn clear_dependencies(&mut self) {
        self.dependencies.clear();
    }

    /// Removes a single dependency registration.
    pub fn remove_dependency(&mut self, index: u32) {
        self.dependencies.remove(&index);
    }
}

/// Updates the leaf at `leaf_index` to `value` and, if the change exceeds the
/// threshold, queues all dependent circuit nodes for recomputation.
pub fn update(
    reactive_circuit: &mut ReactiveCircuit,
    leaf_index: u32,
    value: Vector,
    timestamp: f64,
) {
    let leaf = &mut reactive_circuit.leafs[leaf_index as usize];
    if leaf.set_value(value, timestamp) {
        reactive_circuit.queue.extend(&leaf.dependencies);
    }
}

/// Unconditionally queues all dependent nodes of the leaf at `leaf_index` for
/// recomputation, bypassing the change-threshold check in `set_value`.
pub fn force_invalidate_dependencies(reactive_circuit: &mut ReactiveCircuit, leaf_index: u32) {
    let leaf = &mut reactive_circuit.leafs[leaf_index as usize];
    reactive_circuit.queue.extend(&leaf.dependencies);
}
