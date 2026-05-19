use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::Write;
use std::process::Command;

use itertools::Itertools;
use petgraph::Direction::{Incoming, Outgoing};
use petgraph::{
    algo::toposort,
    stable_graph::{EdgeIndex, NodeIndex, StableGraph},
    visit::EdgeRef,
};
use rayon::prelude::*;

use ndarray::Array1;

use crate::channels::clustering::partitioning;
use crate::circuit::leaf;
use crate::circuit::semiring::{LogProb, Semiring};

use super::{algebraic::AlgebraicCircuit, leaf::Leaf, Vector};

/// A dynamic computation graph where each node contains an `AlgebraicCircuit` for which the result is
/// stored as weight of the incoming edges.
///
/// `S` is the semiring used for evaluation; the default is `LogProb`
/// (log-probability weighted model counting).  Leaf encoded values and edge
/// weights are both stored in `S`'s internal representation.
#[derive(Debug, Clone)]
pub struct ReactiveCircuit<S: Semiring = LogProb> {
    pub structure: StableGraph<AlgebraicCircuit, Vector>,
    pub value_size: usize,
    pub leafs: Vec<Leaf<S>>,
    pub queue: HashSet<u32>,
    pub targets: HashMap<String, NodeIndex>,
    pub partitioning: Vec<usize>,
    /// Nodes grouped by evaluation level: level 0 = leaf ACs (no child circuits),
    /// higher levels depend only on nodes at lower levels.  Cached across updates,
    /// invalidated whenever the graph structure changes.
    topo_levels: Option<Vec<Vec<NodeIndex>>>,
}

impl<S: Semiring> ReactiveCircuit<S> {
    /// Create a new `ReactiveCircuit` with the given `value_size` and set of `leafs`.
    pub fn new(value_size: usize) -> Self {
        assert!(
            value_size > 0,
            "value_size needs to be positive integer greater than 0!"
        );

        ReactiveCircuit {
            structure: StableGraph::new(),
            value_size,
            leafs: Vec::new(),
            queue: HashSet::new(),
            targets: HashMap::new(),
            partitioning: Vec::new(),
            topo_levels: None,
        }
    }

    /// Initialize the ReactiveCircuit from a single sum-product formula.
    pub fn from_sum_product(
        value_size: usize,
        sum_product: &[Vec<u32>],
        target_token: String,
    ) -> Self {
        // Preconditions
        assert!(!sum_product.is_empty(), "sum_product cannot be empty!");
        assert!(!target_token.is_empty(), "target_token cannot be empty!");

        // Initialize ReactiveCircuit with a single AlgebraicCircuit inside
        let mut reactive_circuit = ReactiveCircuit::new(value_size);

        // Create single node and set as target
        let index = reactive_circuit
            .structure
            .add_node(AlgebraicCircuit::from_sum_product(value_size, sum_product));
        reactive_circuit.targets.insert(target_token, index);

        // Make leafs remember this node as dependency
        reactive_circuit.update_dependencies();

        // Queue up the node for recomputation
        reactive_circuit.queue.insert(index.index() as u32);

        // Postconditions
        assert!(reactive_circuit.leafs.len() == sum_product.len());
        assert!(reactive_circuit.structure.node_indices().count() == 1);
        assert!(reactive_circuit.structure.edge_indices().count() == 0);

        reactive_circuit
    }

    /// Adds a new empty target node with the given token.
    ///
    /// **Warning:** the resulting node contains an empty `AlgebraicCircuit`
    /// and will fail the circuit invariants until a formula is added via
    /// `add_sum_product`.  Prefer `add_sum_product` directly.
    pub fn new_target(&mut self, target_token: &str) -> NodeIndex {
        // TODO: Using this function leaves the RC in a bad state (empty AC node)
        // Maybe remove method or require formula?
        self.topo_levels = None;
        assert!(
            !self.targets.contains_key(target_token),
            "Cannot add multiple targets with the same name!"
        );

        let node = self
            .structure
            .add_node(AlgebraicCircuit::new(self.value_size));
        self.targets.insert((*target_token).to_owned(), node);

        node
    }

    /// Appends `sum_product` to the `AlgebraicCircuit` for `target_token`,
    /// creating the target node if it does not yet exist, and registers every
    /// leaf index in `sum_product` as a dependency of that node.
    pub fn add_sum_product(&mut self, sum_product: &[Vec<u32>], target_token: &str) {
        self.topo_levels = None;
        self.check_invariants();

        if !self.targets.contains_key(target_token) {
            self.targets.insert(
                target_token.to_string(),
                self.structure
                    .add_node(AlgebraicCircuit::new(self.value_size)),
            );
        }

        let target_node = self.targets[target_token];
        self.structure[target_node].add_sum_product(sum_product);

        for product in sum_product.iter() {
            for index in product {
                self.set_dependency(*index, &target_node);
            }
        }

        self.queue.insert(target_node.index() as u32);
        self.check_invariants();
    }

    /// Adds a single conjunctive `product` to the target node's `AlgebraicCircuit`
    /// and registers the leaf dependencies.
    pub fn add(&mut self, product: &[u32], target_token: &str) {
        self.check_invariants();
        let target_node = self.targets[target_token];
        self.structure[target_node].add(product);

        for index in product {
            self.set_dependency(*index, &target_node);
        }

        self.queue.insert(target_node.index() as u32);
        self.check_invariants();
    }

    /// Registers `node` as a dependent of the leaf at `index`.
    pub fn set_dependency(&mut self, index: u32, node: &NodeIndex) {
        self.leafs[index as usize].add_dependency(node.index() as u32);
    }

    /// Re-partitions leaves by their current FoC using `boundaries` as bin
    /// edges, then lifts or drops leaves to match the new partitioning.
    pub fn adapt(&mut self, boundaries: &[f64]) {
        self.check_invariants();

        let frequencies = self
            .leafs
            .iter()
            .map(|leaf| leaf.get_frequency())
            .collect::<Vec<f64>>();
        let partitioning = partitioning(&frequencies, boundaries);
        println!("{:?}", partitioning);

        if self.partitioning.is_empty() {
            for (index, &count) in partitioning.iter().enumerate() {
                for _ in 0..count {
                    self.drop_leaf(index as u32);
                }
            }
        } else {
            for (index, &new_count) in partitioning
                .iter()
                .enumerate()
                .take(self.partitioning.len())
            {
                let difference = self.partitioning[index] as i32 - new_count as i32;
                match difference.signum() {
                    -1 => {
                        for _ in 0..-difference {
                            self.lift_leaf(index as u32);
                        }
                    }
                    1 => {
                        for _ in 0..difference {
                            self.drop_leaf(index as u32);
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }

        self.partitioning = partitioning;

        self.check_invariants();
    }

    /// Returns a list of all descendant nodes, grouped by their depth relative to the given `node`.
    ///
    /// The result is a `Vec<Vec<NodeIndex>>`, where the outer vector's index corresponds to the depth
    /// (e.g., index 0 contains all direct children, index 1 contains grandchildren, and so on).
    pub fn get_descendants_by_depth(&self, node: &NodeIndex) -> Vec<Vec<NodeIndex>> {
        let mut descendants_by_depth: Vec<Vec<NodeIndex>> = Vec::new();
        if self.structure.node_weight(*node).is_none() {
            return descendants_by_depth;
        }

        let mut queue: VecDeque<NodeIndex> = VecDeque::new();
        let mut visited: HashSet<NodeIndex> = HashSet::new();

        // Start with the direct children of the root node
        for child in self.structure.neighbors_directed(*node, Outgoing) {
            if visited.insert(child) {
                queue.push_back(child);
            }
        }

        while !queue.is_empty() {
            let level_size = queue.len();
            let current_level_nodes: Vec<NodeIndex> = queue.drain(0..level_size).collect();

            for current_node in &current_level_nodes {
                for child in self.structure.neighbors_directed(*current_node, Outgoing) {
                    if visited.insert(child) {
                        queue.push_back(child);
                    }
                }
            }
            descendants_by_depth.push(current_level_nodes);
        }

        descendants_by_depth
    }

    /// Marks every node in the circuit as outdated by adding all node indices
    /// to the queue, so the next `update` call recomputes the entire circuit.
    pub fn invalidate(&mut self) {
        // Invalidate in a bottom-up fashion so that the update queue can be processed from bottom to top
        let sorted_nodes =
            toposort(&self.structure, None).expect("ReactiveCircuit should be a DAG");
        self.queue
            .extend(sorted_nodes.iter().map(|node| node.index() as u32));
        self.queue = self.queue.iter().unique().cloned().collect();
    }

    /// Remove RC nodes whose AC has no leaves and no memories, cleaning up any
    /// dangling memory columns in peer ACs first.
    pub fn prune(&mut self) {
        self.topo_levels = None;
        let nodes_to_remove: Vec<NodeIndex> = self
            .structure
            .node_indices()
            .filter(|&n| {
                self.structure[n].leafs.is_empty() && self.structure[n].memories.is_empty()
            })
            .collect();

        for node_to_remove in nodes_to_remove {
            if !self.structure.contains_node(node_to_remove) {
                continue;
            }

            let incident_edges: Vec<EdgeIndex> = self
                .structure
                .edges_directed(node_to_remove, Incoming)
                .map(|e| e.id())
                .chain(
                    self.structure
                        .edges_directed(node_to_remove, Outgoing)
                        .map(|e| e.id()),
                )
                .collect();

            let all_nodes: Vec<NodeIndex> = self.structure.node_indices().collect();
            for node_idx in all_nodes {
                if node_idx == node_to_remove {
                    continue;
                }
                let ac = self.structure.node_weight_mut(node_idx).unwrap();
                for &edge in &incident_edges {
                    if let Some(col) = ac.get_memory(edge) {
                        ac.remove_col(col);
                    }
                }
            }

            self.structure.remove_node(node_to_remove);
        }
    }

    /// Ensure that an AlgebraicCircuit with `index` within the ReactiveCircuit has a parent, e.g., to lift a leaf into.
    fn ensure_parent(&mut self, index: NodeIndex) -> Vec<(NodeIndex, EdgeIndex)> {
        self.check_invariants();

        let parents_and_edges: Vec<(NodeIndex, EdgeIndex)> = self
            .structure
            .edges_directed(index, Incoming)
            .map(|edge| (edge.source(), edge.id()))
            .collect();

        if parents_and_edges.is_empty() {
            let parent = self
                .structure
                .add_node(AlgebraicCircuit::new(self.value_size));
            let edge = self.structure.add_edge(
                parent,
                index,
                Array1::from_elem(self.value_size, S::zero()).into_shared(),
            );

            self.queue.insert(parent.index() as u32);

            let ac = self.structure.node_weight_mut(parent).unwrap();
            let mem_col = ac.create_memory(edge);
            ac.push_single(mem_col);

            let tokens_to_update: Vec<String> = self
                .targets
                .iter()
                .filter(|(_, &node_index)| node_index == index)
                .map(|(token, _)| token.clone())
                .collect();
            for token in tokens_to_update {
                self.targets.insert(token, parent);
            }

            return vec![(parent, edge)];
        }

        self.check_invariants();
        parents_and_edges
    }

    /// Get all ancestors of a node, including the node itself.
    fn get_ancestors(&self, node: NodeIndex) -> HashSet<NodeIndex> {
        let mut ancestors = HashSet::new();
        let mut queue = VecDeque::new();

        if self.structure.contains_node(node) {
            queue.push_back(node);
            ancestors.insert(node);
        }

        while let Some(current) = queue.pop_front() {
            for parent in self.structure.neighbors_directed(current, Incoming) {
                if ancestors.insert(parent) {
                    queue.push_back(parent);
                }
            }
        }

        ancestors
    }

    /// Recomputes the dependency set for every leaf by walking all circuit nodes
    /// and collecting the ancestors of any node that contains the leaf.
    pub fn update_dependencies(&mut self) {
        for index in 0..self.leafs.len() as u32 {
            let mut new_dependencies = BTreeSet::new();

            for node in self.structure.node_indices() {
                if self.structure[node].get_leaf(index).is_some() {
                    for ancestor in self.get_ancestors(node) {
                        new_dependencies.insert(ancestor.index() as u32);
                    }
                }
            }

            self.leafs[index as usize].dependencies = new_dependencies;
        }
    }

    /// Lift the leaf with `index` out of its current circuits into its ancestors.
    pub fn lift_leaf(&mut self, index: u32) {
        self.topo_levels = None;
        for dependency in self.leafs[index as usize].get_dependencies() {
            self.check_invariants();

            let node_to_lift: NodeIndex = dependency.into();
            if self
                .structure
                .node_weight(node_to_lift)
                .unwrap()
                .get_leaf(index)
                .is_none()
            {
                continue;
            }

            let parents_and_edges = self.ensure_parent(node_to_lift);
            let ac = self.structure.node_weight_mut(node_to_lift).unwrap();
            let (in_scope_circuit, out_of_scope_circuit) = ac.split(index);

            // Helper: add a freshly-cloned child AC to the graph, rewiring its
            // memory columns to new edges that originate from the new node.
            let reattach = |this: &mut Self, mut circuit: AlgebraicCircuit| -> NodeIndex {
                // Remove the split leaf if present (in-scope circuit still has it).
                if let Some(col) = circuit.get_leaf(index) {
                    circuit.remove_col(col);
                }
                let node = this.structure.add_node(circuit);
                // Remap each memory column: the cloned AC refers to old edge indices
                // that belonged to node_to_lift; create new edges from the new node.
                let old_memories: Vec<(u32, usize)> = this
                    .structure
                    .node_weight(node)
                    .unwrap()
                    .memories
                    .iter()
                    .map(|(&k, &v)| (k, v))
                    .collect();
                for (old_key, col) in old_memories {
                    let old_edge = EdgeIndex::new(old_key as usize);
                    let weight = this.structure.edge_weight(old_edge).unwrap().clone();
                    let target = this.structure.edge_endpoints(old_edge).unwrap().1;
                    let new_edge = this.structure.add_edge(node, target, weight);
                    this.structure
                        .node_weight_mut(node)
                        .unwrap()
                        .remap_memory(col, old_key, new_edge);
                }
                node
            };

            let out_of_scope_node = out_of_scope_circuit.map(|c| reattach(self, c));

            // Count bare minterms ({x} alone) before reattach prunes them.
            // Each bare term contributes P(x) directly to the parent without a child node.
            let in_scope_circuit = in_scope_circuit.expect("in_scope must exist");
            let bare_count = in_scope_circuit
                .get_leaf(index)
                .map(|x_col| {
                    in_scope_circuit
                        .minterms
                        .iter()
                        .filter(|row| row.as_slice() == [x_col])
                        .count()
                })
                .unwrap_or(0);

            let in_scope_node = reattach(self, in_scope_circuit);
            let in_scope_has_proper = !self.structure[in_scope_node].is_empty();
            if !in_scope_has_proper {
                self.structure.remove_node(in_scope_node);
            }

            for (parent, _edge) in parents_and_edges {
                // Remove old edge + memory; get the row that contained it.
                let original_row = self.disconnect(parent, node_to_lift);

                // Columns shared by every new row added for this parent.
                let base_cols: Vec<usize> = self
                    .structure
                    .node_weight(parent)
                    .unwrap()
                    .get_minterm_cols(original_row)
                    .to_vec();
                let leaf_col = self
                    .structure
                    .node_weight_mut(parent)
                    .unwrap()
                    .ensure_leaf(index);

                // One direct row per bare minterm — no child node, just P(x).
                for _ in 0..bare_count {
                    let mut bare_cols = base_cols.clone();
                    bare_cols.push(leaf_col);
                    self.structure
                        .node_weight_mut(parent)
                        .unwrap()
                        .push_minterm(bare_cols);
                }

                // One row for the proper (non-bare) in-scope minterms, wired to the child.
                if in_scope_has_proper {
                    let mut in_scope_cols = base_cols.clone();
                    in_scope_cols.push(leaf_col);
                    let in_scope_row = self
                        .structure
                        .node_weight_mut(parent)
                        .unwrap()
                        .push_minterm(in_scope_cols);
                    self.connect(parent, in_scope_node, in_scope_row);
                    self.queue.insert(in_scope_node.index() as u32);
                }

                if let Some(oos_node) = out_of_scope_node {
                    self.connect(parent, oos_node, original_row);
                    self.queue.insert(oos_node.index() as u32);
                } else {
                    // No out-of-scope rows: the original row is now empty, drop it.
                    self.structure
                        .node_weight_mut(parent)
                        .unwrap()
                        .minterms
                        .remove(original_row);
                }
            }

            self.structure.remove_node(node_to_lift);
        }

        self.update_dependencies();
        self.check_invariants();
    }

    /// Remove the leaf with `index` from every circuit that directly contains
    /// it, pushing its contribution down into descendant circuits.
    pub fn drop_leaf(&mut self, index: u32) {
        self.topo_levels = None;
        self.check_invariants();

        for dependency in self.leafs[index as usize].get_dependencies() {
            let dependency: NodeIndex = dependency.into();
            let leaf_col = match self.structure[dependency].get_leaf(index) {
                Some(col) => col,
                None => continue,
            };

            let rows = self.structure[dependency].minterms_containing_col(leaf_col);
            for row in rows {
                self.handle_leaf_drop_for_product(index, dependency, row);
            }

            self.structure
                .node_weight_mut(dependency)
                .unwrap()
                .remove_col(leaf_col);

            for child in self.structure.neighbors_directed(dependency, Outgoing) {
                self.queue.insert(child.index() as u32);
            }
        }

        self.update_dependencies();
        self.check_invariants();
    }

    /// Push leaf `leaf_index` from `dependency`'s row `row` down into a child:
    /// either multiply it into the child that a sibling memory points to, or
    /// create a new child AC containing only that leaf.
    fn handle_leaf_drop_for_product(&mut self, leaf_index: u32, dependency: NodeIndex, row: usize) {
        let mem_col = self.structure[dependency]
            .get_minterm_cols(row)
            .iter()
            .copied()
            .find(|&c| self.structure[dependency].col_is_memory(c));

        if let Some(col) = mem_col {
            let edge = self.structure[dependency].col_memory_edge(col).unwrap();
            let (_, child) = self.structure.edge_endpoints(edge).unwrap();
            self.structure[child].multiply(leaf_index);
        } else {
            let new_ac = AlgebraicCircuit::from_sum_product(self.value_size, &[vec![leaf_index]]);
            let new_node = self.structure.add_node(new_ac);
            let new_edge = self.structure.add_edge(
                dependency,
                new_node,
                Array1::from_elem(self.value_size, S::zero()).into_shared(),
            );
            let ac = self.structure.node_weight_mut(dependency).unwrap();
            let mem_col = ac.create_memory(new_edge);
            ac.add_col_to_minterm(row, mem_col);
        }
    }

    /// Add a memory column for `child` to the parent's AC row `row`, wiring a
    /// new reactive edge.  Returns the new memory column index.
    pub fn connect(&mut self, parent: NodeIndex, child: NodeIndex, row: usize) -> usize {
        self.topo_levels = None;
        let edge = self.structure.add_edge(
            parent,
            child,
            Array1::from_elem(self.value_size, S::zero()).into_shared(),
        );
        let mem_col = self
            .structure
            .node_weight_mut(parent)
            .unwrap()
            .create_memory(edge);
        self.structure
            .node_weight_mut(parent)
            .unwrap()
            .add_col_to_minterm(row, mem_col);
        mem_col
    }

    /// Remove the edge from `parent` to `child` and its memory column.
    /// Returns the row index that contained the memory (now without it).
    pub fn disconnect(&mut self, parent: NodeIndex, child: NodeIndex) -> usize {
        self.topo_levels = None;
        let edge = self
            .structure
            .edges_connecting(parent, child)
            .map(|e| e.id())
            .next()
            .unwrap();
        let mem_col = self
            .structure
            .node_weight(parent)
            .unwrap()
            .get_memory(edge)
            .unwrap();
        let rows = self
            .structure
            .node_weight(parent)
            .unwrap()
            .minterms_containing_col(mem_col);
        let row = rows[0];
        // Keep the now-empty row alive so the caller can repopulate it.
        self.structure
            .node_weight_mut(parent)
            .unwrap()
            .remove_col_keep_rows(mem_col);
        self.structure.remove_edge(edge);
        row
    }

    /// Update the necessary values within the ReactiveCircuit and its output.
    /// Returns a `HashMap<String, Vector>` where the key is a target token and the value
    /// contains the computed outcome.
    pub fn update(&mut self) -> HashMap<String, Vector> {
        // We collect data to share to the outside world
        let mut target_results = HashMap::new();
        let outdated_nodes = self.queue.clone();
        self.queue.clear();

        // Build level decomposition once; invalidated on structural changes.
        // Level 0 = leaf ACs (no child circuits); level k depends only on levels < k.
        if self.topo_levels.is_none() {
            let topo = toposort(&self.structure, None).expect("ReactiveCircuit should be a DAG");

            // Assign each node its evaluation level (children first = reverse topo order).
            let max_idx = self
                .structure
                .node_indices()
                .map(|n| n.index())
                .max()
                .unwrap_or(0);
            let mut node_level = vec![0usize; max_idx + 1];
            for &node in topo.iter().rev() {
                let child_max = self
                    .structure
                    .neighbors_directed(node, Outgoing)
                    .map(|c| node_level[c.index()])
                    .max();
                node_level[node.index()] = child_max.map_or(0, |l| l + 1);
            }

            let depth = node_level.iter().copied().max().unwrap_or(0);
            let mut levels: Vec<Vec<NodeIndex>> = vec![vec![]; depth + 1];
            for node in self.structure.node_indices() {
                levels[node_level[node.index()]].push(node);
            }
            self.topo_levels = Some(levels);
        }

        // Process levels from 0 upward: within each level all nodes are independent.
        let n_levels = self.topo_levels.as_ref().unwrap().len();
        for lvl in 0..n_levels {
            // Phase 1 — parallel: compute values for every queued node in this level.
            // All reads; the children's edge weights (levels < lvl) are fully written already.
            let level_nodes = self.topo_levels.as_ref().unwrap()[lvl].clone();
            let level_results: Vec<(NodeIndex, Vector)> = level_nodes
                .par_iter()
                .filter(|&&node| outdated_nodes.contains(&(node.index() as u32)))
                .map(|&node| {
                    let result = self.structure[node].evaluate::<S>(self);
                    (node, result)
                })
                .collect();

            // Phase 2 — sequential: write results back to parent edges and target map.
            for (node, result) in level_results {
                for (token, &target_node) in &self.targets {
                    if target_node == node {
                        target_results
                            .insert(token.to_owned(), result.mapv(S::decode).into_shared());
                    }
                }
                let edges: Vec<EdgeIndex> = self
                    .structure
                    .edges_directed(node, Incoming)
                    .map(|e| e.id())
                    .collect();
                for edge in edges {
                    self.structure
                        .edge_weight_mut(edge)
                        .expect("ReactiveCircuit edge was missing!")
                        .assign(&result);
                }
            }
        }

        target_results
    }

    /// Invalidates the entire circuit and then runs `update`, guaranteeing that
    /// all target values are freshly recomputed regardless of queue state.
    pub fn full_update(&mut self) -> HashMap<String, Vector> {
        self.invalidate();
        self.update()
    }

    /// Unpacks a `ProbGradient` result map into `{ target → (wmc, { leaf_name → gradient }) }`.
    ///
    /// Only targets whose result vector length equals `1 + n_leaves` are included;
    /// any other target (e.g. from a plain `LogProb` circuit) is silently skipped.
    pub fn unpack_gradients(
        &self,
        results: &HashMap<String, Vector>,
    ) -> HashMap<String, (f64, HashMap<String, f64>)> {
        let n = self.leafs.len();
        results
            .iter()
            .filter(|(_, vec)| vec.len() == 1 + n)
            .map(|(target, vec)| {
                let wmc = vec[0];
                let gradients = self
                    .leafs
                    .iter()
                    .enumerate()
                    .map(|(i, leaf)| (leaf.name.clone(), vec[i + 1]))
                    .collect();
                (target.clone(), (wmc, gradients))
            })
            .collect()
    }

    /// Runs the reactive update and returns `ProbGradient` results unpacked by leaf name.
    ///
    /// Only recomputes circuits whose inputs have changed since the last call.
    /// The returned map has the form `{ target → (wmc, { leaf_name → gradient }) }`.
    pub fn gradient_update(&mut self) -> HashMap<String, (f64, HashMap<String, f64>)> {
        let results = self.update();
        self.unpack_gradients(&results)
    }

    /// Invalidates the entire circuit, then runs an update and returns
    /// `ProbGradient` results unpacked by leaf name.
    ///
    /// Use this when you need every target recomputed unconditionally.
    /// The returned map has the form `{ target → (wmc, { leaf_name → gradient }) }`.
    pub fn full_gradient_update(&mut self) -> HashMap<String, (f64, HashMap<String, f64>)> {
        let results = self.full_update();
        self.unpack_gradients(&results)
    }

    /// Applies one gradient-descent step to leaf probabilities.
    ///
    /// `gradients` is the per-leaf gradient map for a single target, as returned by
    /// the second element of a `gradient_update()` entry.  `loss` is `∂L/∂P` — the
    /// scalar upstream gradient of the loss with respect to the WMC output (e.g.
    /// `2·(P − target)` for MSE).  The update rule for each fitted leaf is:
    ///
    /// ```text
    /// p_new = clamp(p_i − lr · loss · ∂P/∂p_i,  0,  1)
    /// ```
    ///
    /// When `atoms` is `None` every leaf is updated.  Pass `Some(names)` to
    /// restrict updates to a specific subset of atoms.
    pub fn fit(
        &mut self,
        gradients: &HashMap<String, f64>,
        lr: f64,
        loss: f64,
        atoms: Option<&[String]>,
        timestamp: f64,
    ) {
        let value_size = self.value_size;
        let updates: Vec<(u32, f64)> = (0..self.leafs.len())
            .filter_map(|i| {
                let leaf = &self.leafs[i];
                if let Some(list) = atoms {
                    if !list.iter().any(|a| a == &leaf.name) {
                        return None;
                    }
                }
                gradients.get(&leaf.name).map(|&grad| {
                    let p_i = leaf.get_encoded_value()[0];
                    let p_new = (p_i - lr * loss * grad).clamp(0.0, 1.0);
                    (i as u32, p_new)
                })
            })
            .collect();

        for (idx, p_new) in updates {
            leaf::update(
                self,
                idx,
                Vector::from_elem(value_size, p_new).into_shared(),
                timestamp,
            );
        }
    }

    #[cfg(debug_assertions)]
    pub fn check_invariants(&self) {
        let mut violations = Vec::new();

        // Invariant 1: every RC edge has a memory column in the source AC.
        for edge in self.structure.edge_indices() {
            let (source, target) = self.structure.edge_endpoints(edge).unwrap();
            if self.structure[source].get_memory(edge).is_none() {
                violations.push(format!(
                    "Invariant Violation: Edge {:?} from {:?} to {:?} exists, but source AC is missing memory column.",
                    edge, source, target
                ));
            }
        }

        // Invariant 2: every AC memory column references a valid RC edge.
        for node in self.structure.node_indices() {
            for &key in self.structure[node].memories.keys() {
                let edge = EdgeIndex::new(key as usize);
                if self.structure.edge_weight(edge).is_none() {
                    violations.push(format!(
                        "Invariant Violation: Node {:?} has memory column for edge {:?}, but that edge does not exist.",
                        node, edge
                    ));
                }
            }
        }

        // Invariant 3: every AC has at least one minterm.
        for node in self.structure.node_indices() {
            if self.structure[node].is_empty() {
                violations.push(format!(
                    "Invariant Violation: Node {:?} has an empty algebraic circuit.",
                    node
                ));
            }
        }

        // Invariant 4: every AC has at least one column.
        for node in self.structure.node_indices() {
            if self.structure[node].columns.is_empty() {
                violations.push(format!(
                    "Invariant Violation: Node {:?} has no columns (empty scope).",
                    node
                ));
            }
        }

        if !violations.is_empty() {
            let _ = self.to_svg("invariant_violation.svg", true);
            panic!("Invariant violations found:\n{}", violations.join("\n"));
        }
    }

    #[cfg(not(debug_assertions))]
    pub fn check_invariants(&self) {}

    /// Compile AlgebraicCircuit into dot format text and return as `String`.
    pub fn to_dot_text(&self) -> String {
        let mut dot = String::new();

        // Start the DOT graph
        dot.push_str("digraph ReactiveCircuit {\n");
        dot.push_str("    node [color=\"chartreuse3\" margin=0 penwidth=2];\n");
        dot.push_str("    edge [color=\"gray25\" penwidth=2];\n");

        // Iterate over the nodes
        for node in self.structure.node_indices() {
            let ac = &self.structure[node];
            let scope: Vec<String> = ac
                .columns
                .iter()
                .map(|col| match col {
                    super::algebraic::Column::Leaf(i) => format!("L{}", i),
                    super::algebraic::Column::Memory(k) => format!("M{}", k),
                })
                .collect();
            let node_label = format!(
                "P({}) = ΣΠ\\n{}",
                self.targets
                    .iter()
                    .filter(|(_, v)| **v == node)
                    .map(|(k, _)| k)
                    .join(""),
                scope.join(" "),
            );
            dot.push_str(&format!(
                "    {} [shape=\"circle\" label=\"{}\"];\n",
                node.index(),
                node_label
            ));
        }

        // Iterate over the edges
        for edge in self.structure.edge_indices() {
            let (source, target) = self.structure.edge_endpoints(edge).unwrap();
            dot.push_str(&format!(
                "    {} -> {} [label=\"M{}={:.2}\" decorate=\"true\"];\n",
                source.index(),
                target.index(),
                edge.index(),
                self.structure[edge][0]
            ));
        }

        // End the DOT graph
        dot.push_str("}\n");
        dot
    }

    /// Write out the ReactiveCircuit as dot file at the given `path`.
    pub fn to_dot(&self, path: &str) -> std::io::Result<()> {
        // Translate graph into DOT text
        let dot = self.to_dot_text();

        // Write to disk
        let mut file = File::create(path)?;
        file.write_all(dot.as_bytes())?;

        Ok(())
    }

    /// Write out the ReactiveCircuit as svg file at the given `path`.
    /// If `keep_dot` is set to true, the dot text is written to `path.dot`.
    pub fn to_svg(&self, path: &str, keep_dot: bool) -> std::io::Result<()> {
        // Translate graph into DOT text and write to disk
        let dot_path = if keep_dot {
            path.to_owned() + ".dot"
        } else {
            path.to_owned()
        };
        self.to_dot(&dot_path)?;

        // Compile into SVG using graphviz
        let svg_text = Command::new("dot")
            .args(["-Tsvg", &dot_path])
            .output()
            .expect("Failed to run graphviz!");

        // Pass stdout into new file with SVG content
        let mut file = File::create(path)?;
        file.write_all(&svg_text.stdout)?;
        file.sync_all()?;

        Ok(())
    }

    /// Creates an SVG at the given `path` containing both the ReactiveCircuit as well as all contained
    /// AlgebraicCircuits rendered by Graphviz.
    pub fn to_combined_svg(&self, path: &str) -> std::io::Result<()> {
        // Setup file to write to
        let mut file = File::create(path)?;

        // Describe ReactiveCircuit itself in dot format
        file.write_all(self.to_dot_text().as_bytes())?;

        // Gather dot text for all contained AlgebraicCircuits
        for node in self.structure.node_indices() {
            file.write_all(self.structure[node].to_dot_text().as_bytes())?;
        }

        // Ensure write is complete
        file.sync_all()?;

        // Run gvpack on combined dot text, this is necessary before graphviz/dot
        let packed_dot = Command::new("gvpack")
            .args(["-u", path])
            .output()
            .expect("Failed to run graphviz!");

        // Write packed result to file
        let mut file = File::create(path)?;
        file.write_all(&packed_dot.stdout)?;
        file.sync_all()?;

        // Compile into SVG using graphviz
        let svg_text = Command::new("dot")
            .args(["-Tsvg", path])
            .output()
            .expect("Failed to run graphviz!");

        // Pass stdout into new file with SVG content
        let mut file = File::create(path)?;
        file.write_all(&svg_text.stdout)?;
        file.sync_all()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use ndarray::array;
    use rand::prelude::IndexedRandom;
    use rand::Rng;

    use super::*;
    use std::collections::BTreeSet;

    use crate::channels::manager::Manager;
    use crate::circuit::leaf::update;
    use crate::circuit::semiring::LogProb;

    use super::Vector;

    type TestRC = ReactiveCircuit<LogProb>;
    type TestManager = Manager<LogProb>;

    fn calculate_expected_value(
        sum_of_products: &[Vec<u32>],
        leaf_values: &[Vector],
        value_size: usize,
    ) -> Vector {
        sum_of_products
            .iter()
            .map(|product| {
                product
                    .iter()
                    .map(|&leaf_idx| leaf_values[leaf_idx as usize].clone())
                    .fold(Vector::ones(value_size), |a, b| a * b)
            })
            .fold(Vector::zeros(value_size), |a, b| a + b)
    }

    #[test]
    fn test_randomized_adaptation() {
        let mut rng = rand::rng();
        let value_size = 1;
        let number_leafs = 50;
        let number_products = 250;
        let product_size = 25;
        let simulation_steps = 100;

        // 1. Setup Manager and ReactiveCircuit
        let manager = TestManager::new(value_size);
        let mut reactive_circuit = manager.reactive_circuit.lock().unwrap();

        // 2. Create a large, random formula
        for i in 0..number_leafs {
            reactive_circuit.leafs.push(Leaf::new(
                Vector::from(vec![rng.random_range(0.0..1.0)]),
                0.0,
                &format!("leaf_{}", i),
                i, // leaf_index
            ));
        }

        let mut sum_of_products = Vec::new();
        let leaf_indices: Vec<u32> = (0..number_leafs as u32).collect();
        for _ in 0..number_products {
            let product: Vec<u32> = leaf_indices
                .choose_multiple(&mut rng, product_size)
                .cloned()
                .collect();
            sum_of_products.push(product);
        }

        reactive_circuit.add_sum_product(&sum_of_products, "random_target");
        let _ = reactive_circuit.to_svg("test_randomized_rc.svg", false);

        // 3. Simulation loop
        for step in 0..simulation_steps + 1 {
            // Calculate expected value before any changes in this step
            let leaf_values = reactive_circuit
                .leafs
                .iter()
                .map(|l| l.get_value())
                .collect::<Vec<_>>();
            let expected_value =
                calculate_expected_value(&sum_of_products, &leaf_values, value_size);

            // Check if reactive update results in expected value
            let result = reactive_circuit.update();
            if result.contains_key("random_target") {
                println!(
                    "RC result = {} | Expected = {}",
                    result["random_target"].clone(),
                    expected_value.clone()
                );
                assert!(
                    (result["random_target"].clone() - expected_value.clone())
                        .sum()
                        .abs()
                        < 1e-9
                );
            }

            // Check if full update results in expected value
            let result = reactive_circuit.full_update();
            println!(
                "RC result = {} | Expected = {}",
                result["random_target"].clone(),
                expected_value.clone()
            );
            assert!(
                (result["random_target"].clone() - expected_value.clone())
                    .sum()
                    .abs()
                    < 1e-9
            );

            // Randomly update a leaf
            let leaf_to_update = rng.random_range(0..number_leafs) as u32;
            let new_value = Vector::from(vec![rng.random_range(0.0..1.0)]);
            update(
                &mut reactive_circuit,
                leaf_to_update,
                new_value,
                step as f64,
            );

            // Randomly adapt structure
            let leaf_to_adapt = rng.random_range(0..number_leafs) as u32;
            if rng.random_bool(0.5) {
                println!("Leaf to lift: {}", leaf_to_adapt);
                reactive_circuit.lift_leaf(leaf_to_adapt);
            } else {
                println!("Leaf to drop: {}", leaf_to_adapt);
                reactive_circuit.drop_leaf(leaf_to_adapt);
            }
        }
    }

    #[test]
    fn test_bare_minterm_lift() {
        // Formula: x + x*a = {0} + {0,1}
        // Lifting leaf 0 (x) must preserve the bare {x} term in the parent AC.
        // P(x)=0.5, P(a)=0.4 → value = 0.5 + 0.5*0.4 = 0.7
        let mut rc = TestRC::new(1);
        rc.leafs.push(Leaf::new(array![0.5].into(), 0.0, "x", 0));
        rc.leafs.push(Leaf::new(array![0.4].into(), 0.0, "a", 1));
        rc.add_sum_product(&[vec![0], vec![0, 1]], "test");

        let expected = 0.5_f64 + 0.5 * 0.4;

        let v_before = rc.full_update()["test"][0];
        assert!(
            (v_before - expected).abs() < 1e-9,
            "before lift: {v_before} != {expected}"
        );

        rc.lift_leaf(0);

        let v_after = rc.full_update()["test"][0];
        assert!(
            (v_after - expected).abs() < 1e-9,
            "after lift: {v_after} != {expected}"
        );
    }

    #[test]
    fn test_rc() -> std::io::Result<()> {
        let manager = TestManager::new(1);
        let reactive_circuit = &mut manager.reactive_circuit.lock().unwrap();

        reactive_circuit
            .leafs
            .push(Leaf::new(Vector::ones(1), 0.0, "", 0));
        reactive_circuit
            .leafs
            .push(Leaf::new(Vector::ones(1), 0.0, "", 1));
        reactive_circuit
            .leafs
            .push(Leaf::new(Vector::ones(1), 0.0, "", 2));

        reactive_circuit.add_sum_product(&[vec![0, 1], vec![0, 2]], "test");

        assert_eq!(reactive_circuit.leafs.len(), 3);
        assert_eq!(reactive_circuit.structure.node_count(), 1);
        // Matrix AC: 2 minterms, 3 columns (leaves 0,1,2)
        let ac = reactive_circuit.structure.node_weight(0.into()).unwrap();
        assert_eq!(ac.minterms.len(), 2);
        assert_eq!(ac.columns.len(), 3);
        assert!(reactive_circuit
            .leafs
            .iter()
            .all(|leaf| leaf.get_dependencies().len() == 1));
        assert!(reactive_circuit
            .leafs
            .iter()
            .all(|leaf| leaf.get_dependencies() == BTreeSet::from_iter(vec![0])));

        let results = reactive_circuit.update();
        let value = results
            .get("test")
            .expect("The key 'test' was not found in the results")
            .clone();
        reactive_circuit.to_combined_svg("output/test/test_rc_original.svg")?;

        // Structural changes require updates
        // Partial and full updates always gives the same result
        reactive_circuit.lift_leaf(0);
        reactive_circuit.to_combined_svg("output/test/test_rc_lift_l0_rc.svg")?;
        assert_eq!(
            reactive_circuit
                .full_update()
                .get("test")
                .expect("The test target was not found in the RC!"),
            &value
        );

        reactive_circuit.drop_leaf(0);
        reactive_circuit.to_combined_svg("output/test/test_rc_lift_drop_l0_rc.svg")?;
        assert_eq!(
            reactive_circuit
                .full_update()
                .get("test")
                .expect("The test target was not found in the RC!"),
            &value
        );

        reactive_circuit.drop_leaf(0);
        reactive_circuit.to_combined_svg("output/test/test_rc_lift_drop_drop_l0_rc.svg")?;
        assert_eq!(
            reactive_circuit
                .full_update()
                .get("test")
                .expect("The test target was not found in the RC!"),
            &value
        );

        Ok(())
    }
}
