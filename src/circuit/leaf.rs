use std::collections::BTreeSet;
use std::marker::PhantomData;

use ndarray::{Array1, ArrayView1};

use crate::channels::FoCEstimator;

use super::reactive::ReactiveCircuit;
use super::semiring::{LogProb, Semiring};
use super::Vector;

/// A time-dynamic input node in the reactive circuit.
///
/// `S` is the semiring used for evaluation; values are stored in `S`'s internal
/// representation so that `get_encoded_value` is a zero-copy view.
/// The default semiring is `LogProb` (log-probability weighted model counting),
/// which keeps `encoded_value` in log-space.
#[derive(Clone, Debug)]
pub struct Leaf<S: Semiring = LogProb> {
    encoded_value: Vector,
    frequency: f64,
    cluster: i32,
    leaf_index: usize,
    foc_estimator: FoCEstimator,
    pub name: String,
    pub dependencies: BTreeSet<u32>,
    _s: PhantomData<S>,
}

impl<S: Semiring> Leaf<S> {
    /// Creates a new leaf.  `value` is a raw probability vector; it is
    /// encoded via `S::encode_leaf_vec` before storage.
    ///
    /// `leaf_index` is the leaf's position in the owning circuit's leaf array;
    /// it is used by semirings such as `ProbGradient` to plant the gradient seed.
    pub fn new(value: Vector, frequency: f64, name: &str, leaf_index: usize) -> Self {
        Self {
            encoded_value: S::encode_leaf_vec(value.view(), leaf_index).into_shared(),
            frequency,
            cluster: 0,
            leaf_index,
            foc_estimator: FoCEstimator::new(frequency),
            name: name.to_owned(),
            dependencies: BTreeSet::new(),
            _s: PhantomData,
        }
    }

    /// Creates a new leaf from a value that is **already in the semiring's
    /// internal representation** (i.e. no encoding is applied).
    /// Use this when the encoded value was computed directly, e.g. via `S::negate`.
    pub fn new_encoded(
        encoded_value: Vector,
        frequency: f64,
        name: &str,
        leaf_index: usize,
    ) -> Self {
        Self {
            encoded_value,
            frequency,
            cluster: 0,
            leaf_index,
            foc_estimator: FoCEstimator::new(frequency),
            name: name.to_owned(),
            dependencies: BTreeSet::new(),
            _s: PhantomData,
        }
    }

    /// Re-encodes this leaf for a new `value_size`.
    ///
    /// Called by `Resin::compile` when a semiring (e.g. `ProbGradient`) requires
    /// `value_size` to be derived from the total leaf count rather than set upfront.
    pub fn resize_for_value_size(&mut self, value_size: usize) {
        let p = S::decode(self.encoded_value[0]);
        let raw = Array1::from_elem(value_size, p).into_shared();
        self.encoded_value = S::encode_leaf_vec(raw.view(), self.leaf_index).into_shared();
    }

    /// Returns a zero-copy view of the stored encoded value.
    pub fn get_encoded_value(&self) -> ArrayView1<'_, f64> {
        self.encoded_value.view()
    }

    /// Returns the probability vector, decoded from the internal representation.
    pub fn get_value(&self) -> Vector {
        self.encoded_value.mapv(S::decode).into_shared()
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
    /// the current value by more than `threshold` in the encoded representation.
    ///
    /// Returns `true` when the value changed (dependent circuits should be
    /// re-queued), `false` when the change was below threshold.
    pub fn set_value(&mut self, value: Vector, timestamp: f64, threshold: f64) -> bool {
        let encoded = S::encode_leaf_vec(value.view(), self.leaf_index);
        if !self.encoded_value.abs_diff_eq(&encoded, threshold) {
            self.encoded_value = encoded.into_shared();
            self.frequency = self.foc_estimator.update(timestamp);
            true
        } else {
            false
        }
    }

    /// Sets the cluster label and returns the signed step `old − new`.
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
/// circuit's `update_threshold`, queues all dependent circuit nodes for recomputation.
pub fn update<S: Semiring>(
    reactive_circuit: &mut ReactiveCircuit<S>,
    leaf_index: u32,
    value: Vector,
    timestamp: f64,
) {
    let value = S::expand_input(value, reactive_circuit.value_size);
    let threshold = reactive_circuit.update_threshold;
    let leaf = &mut reactive_circuit.leafs[leaf_index as usize];
    if leaf.set_value(value, timestamp, threshold) {
        reactive_circuit.queue.extend(&leaf.dependencies);
    }
}

/// Unconditionally queues all dependent nodes of the leaf at `leaf_index` for
/// recomputation, bypassing the change-threshold check in `set_value`.
pub fn force_invalidate_dependencies<S: Semiring>(
    reactive_circuit: &mut ReactiveCircuit<S>,
    leaf_index: u32,
) {
    let leaf = &mut reactive_circuit.leafs[leaf_index as usize];
    reactive_circuit.queue.extend(&leaf.dependencies);
}
