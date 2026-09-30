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
use rustc_hash::FxHashSet;

use ndarray::Array1;

use crate::channels::clustering::partitioning;
use crate::circuit::leaf;
use crate::circuit::semiring::{LogProb, Semiring};

use super::{
    algebraic::{AlgebraicCircuit, Column},
    leaf::Leaf,
    Vector,
};

/// How `lift_leaf`/`drop_leaf` restructure the circuit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Topology {
    /// Merge nodes with identical polynomials, so sub-circuits are shared across parents and targets.
    #[default]
    Dag,
    /// Every node has its own sub-circuits.
    Tree,
}

/// A variable of a node's polynomial, with memories identified by their child node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Var {
    Leaf(u32),
    Child(u32),
}

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
    /// Node indices pending recomputation (FxHash: small internal keys on the hot path).
    pub queue: FxHashSet<u32>,
    pub targets: HashMap<String, NodeIndex>,
    pub partitioning: Vec<usize>,
    /// Minimum change in encoded leaf value required to trigger recomputation of
    /// dependent nodes.  Defaults to `1e-3`.
    pub update_threshold: f64,
    /// Cached evaluation levels (0 = ACs without children); reset on structural change.
    topo_levels: Option<Vec<Vec<NodeIndex>>>,
}

impl<S: Semiring> ReactiveCircuit<S> {
    /// Create an empty `ReactiveCircuit` with the given `value_size`.
    pub fn new(value_size: usize) -> Self {
        assert!(
            value_size > 0,
            "value_size needs to be positive integer greater than 0!"
        );

        ReactiveCircuit {
            structure: StableGraph::new(),
            value_size,
            leafs: Vec::new(),
            queue: FxHashSet::default(),
            targets: HashMap::new(),
            partitioning: Vec::new(),
            update_threshold: 1e-3,
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

    /// Adds an empty target node. Fails invariants until `add_sum_product` fills it; prefer that instead.
    pub fn new_target(&mut self, target_token: &str) -> NodeIndex {
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
    pub fn adapt(&mut self, boundaries: &[f64], topology: Topology) {
        self.check_invariants();

        let frequencies = self
            .leafs
            .iter()
            .map(|leaf| leaf.get_frequency())
            .collect::<Vec<f64>>();
        let partitioning = partitioning(&frequencies, boundaries);

        if self.partitioning.is_empty() {
            // Only relative levels matter, so move every leaf relative to the
            // median band: the fewest lifts and drops for the same structure.
            let mut sorted = partitioning.clone();
            sorted.sort_unstable();
            let median = sorted.get(sorted.len() / 2).copied().unwrap_or(0) as i32;
            for (index, &band) in partitioning.iter().enumerate() {
                let moves = band as i32 - median;
                for _ in 0..moves.abs() {
                    if moves < 0 {
                        self.lift_leaf(index as u32, topology);
                    } else {
                        self.drop_leaf(index as u32, topology);
                    }
                }
            }
        } else {
            for (index, &new_count) in partitioning
                .iter()
                .enumerate()
                .take(self.partitioning.len())
            {
                let difference = self.partitioning[index] as i32 - new_count as i32;
                for _ in 0..difference.abs() {
                    if difference < 0 {
                        self.lift_leaf(index as u32, topology);
                    } else {
                        self.drop_leaf(index as u32, topology);
                    }
                }
            }
        }

        self.partitioning = partitioning;

        self.check_invariants();
    }

    /// Descendants of `node` grouped by depth (index 0 = direct children).
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
        self.queue.extend(
            self.structure
                .node_indices()
                .map(|node| node.index() as u32),
        );
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
            return vec![self.add_root(index)];
        }

        self.check_invariants();
        parents_and_edges
    }

    /// Add a new parent `P = node` above `node` and move all targets of `node` to it.
    fn add_root(&mut self, node: NodeIndex) -> (NodeIndex, EdgeIndex) {
        self.topo_levels = None;
        let parent = self
            .structure
            .add_node(AlgebraicCircuit::new(self.value_size));
        let edge = self.structure.add_edge(
            parent,
            node,
            Array1::from_elem(self.value_size, S::zero()).into_shared(),
        );

        self.queue.insert(parent.index() as u32);

        let ac = self.structure.node_weight_mut(parent).unwrap();
        let mem_col = ac.create_memory(edge);
        ac.push_single(mem_col);

        for target in self.targets.values_mut() {
            if *target == node {
                *target = parent;
            }
        }

        (parent, edge)
    }

    /// Add `circuit` as a new node. Its memory columns still refer to another
    /// node's outgoing edges; each is copied to a new edge from the new node.
    fn add_node_with_copied_edges(&mut self, circuit: AlgebraicCircuit) -> NodeIndex {
        self.topo_levels = None;
        let node = self.structure.add_node(circuit);
        let old_memories: Vec<(u32, usize)> = self.structure[node]
            .memories
            .iter()
            .map(|(&k, &v)| (k, v))
            .collect();
        for (old_key, col) in old_memories {
            let old_edge = EdgeIndex::new(old_key as usize);
            let weight = self.structure[old_edge].clone();
            let target = self.structure.edge_endpoints(old_edge).unwrap().1;
            let new_edge = self.structure.add_edge(node, target, weight);
            self.structure[node].remap_memory(col, old_key, new_edge);
        }
        node
    }

    /// Point `edge` at `new_target`, keeping its source's memory column and weight.
    fn retarget_edge(&mut self, edge: EdgeIndex, new_target: NodeIndex) -> EdgeIndex {
        self.topo_levels = None;
        let source = self.structure.edge_endpoints(edge).unwrap().0;
        let weight = self.structure[edge].clone();
        let new_edge = self.structure.add_edge(source, new_target, weight);
        let ac = &mut self.structure[source];
        let col = ac.get_memory(edge).unwrap();
        ac.remap_memory(col, edge.index() as u32, new_edge);
        self.structure.remove_edge(edge);
        new_edge
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
        let mut dependencies = vec![BTreeSet::new(); self.leafs.len()];

        for node in self.structure.node_indices() {
            if self.structure[node].leafs.is_empty() {
                continue;
            }
            let ancestors = self.get_ancestors(node);
            for &index in self.structure[node].leafs.keys() {
                dependencies[index as usize].extend(ancestors.iter().map(|a| a.index() as u32));
            }
        }

        for (leaf, deps) in self.leafs.iter_mut().zip(dependencies) {
            leaf.dependencies = deps;
        }
    }

    /// Lift the leaf with `index` out of its current circuits into its ancestors.
    pub fn lift_leaf(&mut self, index: u32, topology: Topology) {
        self.topo_levels = None;
        // Parents first: the leaf only moves into already processed parents,
        // so it goes up exactly one level.
        for node_to_lift in self.nodes_with_leaf(index, true) {
            self.check_invariants();

            // `node_to_lift` is removed below, so a target on it needs its own root
            // (`ensure_parent` only adds one if there are no parents yet).
            if self.targets.values().any(|&t| t == node_to_lift)
                && self
                    .structure
                    .neighbors_directed(node_to_lift, Incoming)
                    .next()
                    .is_some()
            {
                self.add_root(node_to_lift);
            }

            let parents_and_edges = self.ensure_parent(node_to_lift);
            let ac = self.structure.node_weight_mut(node_to_lift).unwrap();
            let (in_scope_circuit, out_of_scope_circuit) = ac.split(index);

            // Add a split-off circuit (without the lifted leaf) as a new node.
            let reattach = |this: &mut Self, mut circuit: AlgebraicCircuit| -> NodeIndex {
                if let Some(col) = circuit.get_leaf(index) {
                    circuit.remove_col(col);
                }
                this.add_node_with_copied_edges(circuit)
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

        self.group_all_rows();
        if topology == Topology::Dag {
            self.merge_equivalent_nodes();
        }
        self.update_dependencies();
        self.check_invariants();
    }

    /// Remove the leaf with `index` from every circuit that directly contains
    /// it, pushing its contribution down into descendant circuits.
    pub fn drop_leaf(&mut self, index: u32, topology: Topology) {
        self.topo_levels = None;
        self.check_invariants();

        // In DAG mode, all rows without a child share one new `{leaf}` node.
        let mut leaf_node = None;
        // Children first: the leaf only moves into already processed children,
        // so it goes down exactly one level.
        for dependency in self.nodes_with_leaf(index, false) {
            let leaf_col = self.structure[dependency].get_leaf(index).unwrap();

            let rows = self.structure[dependency].minterms_containing_col(leaf_col);
            for row in rows {
                let shared = (topology == Topology::Dag).then_some(&mut leaf_node);
                self.handle_leaf_drop_for_product(index, dependency, row, shared);
            }

            self.structure
                .node_weight_mut(dependency)
                .unwrap()
                .remove_col(leaf_col);

            for child in self.structure.neighbors_directed(dependency, Outgoing) {
                self.queue.insert(child.index() as u32);
            }
        }

        self.group_all_rows();
        if topology == Topology::Dag {
            self.merge_equivalent_nodes();
        }
        self.update_dependencies();
        self.check_invariants();
    }

    /// Nodes that directly contain leaf `index`, in topological order
    /// (parents before children if `parents_first`, otherwise reversed).
    fn nodes_with_leaf(&self, index: u32, parents_first: bool) -> Vec<NodeIndex> {
        let mut nodes: Vec<NodeIndex> = toposort(&self.structure, None)
            .expect("ReactiveCircuit should be a DAG")
            .into_iter()
            .filter(|&node| self.structure[node].get_leaf(index).is_some())
            .collect();
        if !parents_first {
            nodes.reverse();
        }
        nodes
    }

    /// Merge all nodes whose polynomials are identical, bottom-up, so that
    /// merged children can make their parents identical too. Returns the
    /// number of removed nodes. Leaf dependencies must be updated afterwards.
    ///
    /// Only identically structured nodes merge; `lift_leaf` and `drop_leaf` run
    /// `group_rows` first, so the structure depends only on the leaf levels.
    pub fn merge_equivalent_nodes(&mut self) -> usize {
        let order = toposort(&self.structure, None).expect("ReactiveCircuit should be a DAG");
        let mut seen: HashMap<Vec<Vec<Var>>, NodeIndex> = HashMap::new();
        let mut merged = 0;

        for &node in order.iter().rev() {
            let key = self.polynomial_key(node);
            match seen.get(&key) {
                Some(&survivor) => {
                    self.merge_into(node, survivor);
                    merged += 1;
                }
                None => {
                    seen.insert(key, node);
                }
            }
        }

        if merged > 0 {
            self.topo_levels = None;
        }
        merged
    }

    /// The rows of `node`'s polynomial as sorted variables, in sorted order.
    fn polynomial_key(&self, node: NodeIndex) -> Vec<Vec<Var>> {
        let ac = &self.structure[node];
        let vars: Vec<Var> = ac
            .columns
            .iter()
            .map(|col| match col {
                Column::Leaf(i) => Var::Leaf(*i),
                Column::Memory(k) => {
                    let (_, child) = self
                        .structure
                        .edge_endpoints(EdgeIndex::new(*k as usize))
                        .unwrap();
                    Var::Child(child.index() as u32)
                }
            })
            .collect();
        let mut rows: Vec<Vec<Var>> = ac
            .minterms
            .iter()
            .map(|row| {
                let mut vars: Vec<Var> = row.iter().map(|&c| vars[c]).collect();
                vars.sort_unstable();
                vars
            })
            .collect();
        rows.sort_unstable();
        rows
    }

    /// Replace `victim` by the equivalent `survivor` in all parents, targets and the queue.
    fn merge_into(&mut self, victim: NodeIndex, survivor: NodeIndex) {
        let incoming: Vec<EdgeIndex> = self
            .structure
            .edges_directed(victim, Incoming)
            .map(|e| e.id())
            .collect();
        for edge in incoming {
            self.retarget_edge(edge, survivor);
        }
        for target in self.targets.values_mut() {
            if *target == victim {
                *target = survivor;
            }
        }
        if self.queue.remove(&(victim.index() as u32)) {
            self.queue.insert(survivor.index() as u32);
        }
        self.structure.remove_node(victim);
    }

    /// Group rows in every node, parents first. See `group_rows`.
    fn group_all_rows(&mut self) {
        let order = toposort(&self.structure, None).expect("ReactiveCircuit should be a DAG");
        for node in order {
            if self.structure.contains_node(node) {
                self.group_rows(node);
            }
        }
    }

    /// Combine rows of `node` with the same leaves and one child each into a
    /// single row, `L·M(C1) + L·M(C2) → L·M(S)`, where `S` holds the rows of all
    /// those children and is grouped in turn. Children left without parents or
    /// targets are removed. Rows without a child are kept as they are.
    fn group_rows(&mut self, node: NodeIndex) {
        let groups: Vec<Vec<usize>> = {
            let ac = &self.structure[node];
            let mut by_leaves: HashMap<Vec<u32>, Vec<usize>> = HashMap::new();
            for (row, cols) in ac.minterms.iter().enumerate() {
                if cols.iter().filter(|&&c| ac.col_is_memory(c)).count() != 1 {
                    continue;
                }
                let mut leaves: Vec<u32> = cols
                    .iter()
                    .filter_map(|&c| match ac.columns[c] {
                        Column::Leaf(i) => Some(i),
                        Column::Memory(_) => None,
                    })
                    .collect();
                leaves.sort_unstable();
                by_leaves.entry(leaves).or_default().push(row);
            }
            by_leaves
                .into_values()
                .filter(|rows| rows.len() > 1)
                .collect()
        };
        if groups.is_empty() {
            return;
        }
        self.topo_levels = None;

        let mut removed_rows = HashSet::new();
        let mut old_edges = Vec::new();
        let mut sums = Vec::new();
        for rows in groups {
            let edges: Vec<EdgeIndex> = rows
                .iter()
                .map(|&row| {
                    let ac = &self.structure[node];
                    let col = ac.minterms[row]
                        .iter()
                        .copied()
                        .find(|&c| ac.col_is_memory(c))
                        .unwrap();
                    ac.col_memory_edge(col).unwrap()
                })
                .collect();

            let sum = self
                .structure
                .add_node(AlgebraicCircuit::new(self.value_size));
            for &edge in &edges {
                let child = self.structure.edge_endpoints(edge).unwrap().1;
                self.append_rows(sum, child);
            }

            // The first row now points to the sum; the others are removed below.
            let new_edge = self.structure.add_edge(
                node,
                sum,
                Array1::from_elem(self.value_size, S::zero()).into_shared(),
            );
            let ac = &mut self.structure[node];
            let old_col = ac.get_memory(edges[0]).unwrap();
            let new_col = ac.create_memory(new_edge);
            let row = &mut ac.minterms[rows[0]];
            row.retain(|&c| c != old_col);
            row.push(new_col);
            row.sort_unstable();

            removed_rows.extend(rows[1..].iter().copied());
            old_edges.extend(edges);
            sums.push(sum);
        }

        for edge in old_edges {
            let child = self.structure.edge_endpoints(edge).unwrap().1;
            let ac = &mut self.structure[node];
            let col = ac.get_memory(edge).unwrap();
            ac.remove_col_keep_rows(col);
            self.structure.remove_edge(edge);
            if self
                .structure
                .neighbors_directed(child, Incoming)
                .next()
                .is_none()
                && !self.targets.values().any(|&t| t == child)
            {
                self.queue.remove(&(child.index() as u32));
                self.structure.remove_node(child);
            }
        }

        let mut row = 0;
        self.structure[node].minterms.retain(|_| {
            let keep = !removed_rows.contains(&row);
            row += 1;
            keep
        });

        self.queue.insert(node.index() as u32);
        for sum in sums {
            self.queue.insert(sum.index() as u32);
            self.group_rows(sum);
        }
    }

    /// Append a copy of `source`'s rows to `target`, with new edges to their children.
    fn append_rows(&mut self, target: NodeIndex, source: NodeIndex) {
        let source_ac = self.structure[source].clone();
        for row in &source_ac.minterms {
            let mut cols = Vec::with_capacity(row.len());
            for &c in row {
                match source_ac.columns[c] {
                    Column::Leaf(i) => cols.push(self.structure[target].ensure_leaf(i)),
                    Column::Memory(k) => {
                        let old_edge = EdgeIndex::new(k as usize);
                        let weight = self.structure[old_edge].clone();
                        let child = self.structure.edge_endpoints(old_edge).unwrap().1;
                        let edge = self.structure.add_edge(target, child, weight);
                        cols.push(self.structure[target].create_memory(edge));
                    }
                }
            }
            self.structure[target].push_minterm(cols);
        }
    }

    /// Push leaf `leaf_index` from `dependency`'s row `row` down into a child:
    /// either multiply it into the child that a sibling memory points to, or
    /// create a new child AC containing only that leaf. If `leaf_node` is given,
    /// that child is created once and reused across calls.
    fn handle_leaf_drop_for_product(
        &mut self,
        leaf_index: u32,
        dependency: NodeIndex,
        row: usize,
        leaf_node: Option<&mut Option<NodeIndex>>,
    ) {
        let mem_col = self.structure[dependency]
            .get_minterm_cols(row)
            .iter()
            .copied()
            .find(|&c| self.structure[dependency].col_is_memory(c));

        if let Some(col) = mem_col {
            let edge = self.structure[dependency].col_memory_edge(col).unwrap();
            let (_, mut child) = self.structure.edge_endpoints(edge).unwrap();
            // A child with other parents or targets would change for them too, so copy it first.
            if self.structure.edges_directed(child, Incoming).count() > 1
                || self.targets.values().any(|&t| t == child)
            {
                child = self.add_node_with_copied_edges(self.structure[child].clone());
                self.retarget_edge(edge, child);
            }
            self.structure[child].multiply(leaf_index);
        } else {
            let value_size = self.value_size;
            let leaf_ac = || AlgebraicCircuit::from_sum_product(value_size, &[vec![leaf_index]]);
            let new_node = match leaf_node {
                Some(slot) => *slot.get_or_insert_with(|| self.structure.add_node(leaf_ac())),
                None => self.structure.add_node(leaf_ac()),
            };
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
        let outdated_nodes = std::mem::take(&mut self.queue);

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
            let level_nodes = &self.topo_levels.as_ref().unwrap()[lvl];
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
                        target_results.insert(
                            token.to_owned(),
                            S::decode_vec(result.to_owned()).into_shared(),
                        );
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

    /// `update()` followed by `unpack_gradients`.
    pub fn gradient_update(&mut self) -> HashMap<String, (f64, HashMap<String, f64>)> {
        let results = self.update();
        self.unpack_gradients(&results)
    }

    /// `full_update()` followed by `unpack_gradients`.
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

        // Invariant 5: every memory column appears in exactly one minterm
        // (`disconnect` and the per-edge rewiring in `lift_leaf` rely on this).
        for node in self.structure.node_indices() {
            let ac = &self.structure[node];
            for &col in ac.memories.values() {
                let count = ac.minterms.iter().filter(|row| row.contains(&col)).count();
                if count != 1 {
                    violations.push(format!(
                        "Invariant Violation: Node {:?} uses memory column {} in {} minterms.",
                        node, col, count
                    ));
                }
            }
        }

        // Invariant 6: every minterm has at most one memory column
        // (`group_rows` and `handle_leaf_drop_for_product` rely on this).
        for node in self.structure.node_indices() {
            let ac = &self.structure[node];
            for (row, cols) in ac.minterms.iter().enumerate() {
                let count = cols.iter().filter(|&&c| ac.col_is_memory(c)).count();
                if count > 1 {
                    violations.push(format!(
                        "Invariant Violation: Node {:?} minterm {} has {} memory columns.",
                        node, row, count
                    ));
                }
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

        // Iterate over the nodes, labelled with their targets and polynomial
        for node in self.structure.node_indices() {
            let ac = &self.structure[node];
            let names: Vec<String> = ac
                .columns
                .iter()
                .map(|col| match col {
                    Column::Leaf(i) => match self.leafs.get(*i as usize) {
                        Some(leaf) if !leaf.name.is_empty() => leaf.name.clone(),
                        _ => format!("L{}", i),
                    },
                    Column::Memory(k) => format!("M{}", k),
                })
                .collect();
            let polynomial = ac
                .minterms
                .iter()
                .map(|row| row.iter().map(|&c| names[c].as_str()).join("·"))
                .join(" + ");
            let mut targets: Vec<&str> = self
                .targets
                .iter()
                .filter(|(_, v)| **v == node)
                .map(|(k, _)| k.as_str())
                .collect();
            targets.sort_unstable();
            let node_label = if targets.is_empty() {
                polynomial
            } else {
                format!("{}\\n{}", targets.join(", "), polynomial)
            };
            dot.push_str(&format!(
                "    {} [shape=\"box\" style=\"rounded\" label=\"{}\"];\n",
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
                S::decode(self.structure[edge][0])
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

    /// Several targets drawn from a shared pool of products, checked against
    /// their flat formulas while leaves are updated and lifted/dropped at random.
    fn run_randomized_adaptation(topology: Topology) {
        let mut rng = rand::rng();
        let value_size = 1;
        let number_leafs = 30;
        let pool_size = 60;
        let products_per_target = 30;
        let product_size = 6;
        let targets = ["target_a", "target_b", "target_c"];
        let simulation_steps = 100;

        let manager = TestManager::new(value_size);
        let mut reactive_circuit = manager.reactive_circuit.lock().unwrap();

        for i in 0..number_leafs {
            reactive_circuit.leafs.push(Leaf::new(
                Vector::from(vec![rng.random_range(0.0..1.0)]),
                0.0,
                &format!("leaf_{}", i),
                i,
            ));
        }

        let leaf_indices: Vec<u32> = (0..number_leafs as u32).collect();
        let pool: Vec<Vec<u32>> = (0..pool_size)
            .map(|_| {
                leaf_indices
                    .choose_multiple(&mut rng, product_size)
                    .cloned()
                    .collect()
            })
            .collect();
        let formulas: Vec<Vec<Vec<u32>>> = targets
            .iter()
            .map(|_| {
                pool.choose_multiple(&mut rng, products_per_target)
                    .cloned()
                    .collect()
            })
            .collect();
        for (target, formula) in targets.iter().zip(&formulas) {
            reactive_circuit.add_sum_product(formula, target);
        }

        for step in 0..simulation_steps + 1 {
            let leaf_values = reactive_circuit
                .leafs
                .iter()
                .map(|l| l.get_value())
                .collect::<Vec<_>>();

            let partial = reactive_circuit.update();
            let full = reactive_circuit.full_update();
            for (target, formula) in targets.iter().zip(&formulas) {
                let expected = calculate_expected_value(formula, &leaf_values, value_size);
                if let Some(result) = partial.get(*target) {
                    assert!(
                        (result - &expected).sum().abs() < 1e-9,
                        "step {step}, {target}: update {result} != {expected}"
                    );
                }
                assert!(
                    (&full[*target] - &expected).sum().abs() < 1e-9,
                    "step {step}, {target}: full_update {} != {expected}",
                    full[*target]
                );
            }

            let leaf_to_update = rng.random_range(0..number_leafs) as u32;
            let new_value = Vector::from(vec![rng.random_range(0.0..1.0)]);
            update(
                &mut reactive_circuit,
                leaf_to_update,
                new_value,
                step as f64,
            );

            let leaf_to_adapt = rng.random_range(0..number_leafs) as u32;
            if rng.random_bool(0.5) {
                reactive_circuit.lift_leaf(leaf_to_adapt, topology);
            } else {
                reactive_circuit.drop_leaf(leaf_to_adapt, topology);
            }
        }

        println!(
            "{topology:?}: {} nodes, {} edges",
            reactive_circuit.structure.node_count(),
            reactive_circuit.structure.edge_count()
        );
    }

    #[test]
    fn test_randomized_adaptation_tree() {
        run_randomized_adaptation(Topology::Tree);
    }

    #[test]
    fn test_randomized_adaptation_dag() {
        run_randomized_adaptation(Topology::Dag);
    }

    /// Two targets with the same formula collapse into one node.
    #[test]
    fn test_identical_targets_merge() {
        let mut rc = TestRC::new(1);
        rc.leafs.push(Leaf::new(array![0.5].into(), 0.0, "x", 0));
        rc.leafs.push(Leaf::new(array![0.4].into(), 0.0, "y", 1));
        rc.leafs.push(Leaf::new(array![0.3].into(), 0.0, "z", 2));
        rc.add_sum_product(&[vec![0, 1], vec![2]], "a");
        rc.add_sum_product(&[vec![0, 1], vec![2]], "b");
        let expected = 0.5 * 0.4 + 0.3;

        rc.lift_leaf(0, Topology::Dag);

        assert_eq!(rc.targets["a"], rc.targets["b"]);
        let result = rc.full_update();
        assert!((result["a"][0] - expected).abs() < 1e-9);
        assert!((result["b"][0] - expected).abs() < 1e-9);
    }

    /// `a = x·y + x·z` and `b = w·x·y + w·x·z` start as separate circuits.
    /// After lifting `x` and `w`, both share the node for `y + z`, and dropping
    /// `x` again (copy-on-write, then merge) keeps it shared.
    #[test]
    fn test_targets_grow_together() {
        let mut rc = TestRC::new(1);
        for (i, (name, p)) in [("x", 0.5), ("y", 0.4), ("z", 0.3), ("w", 0.8)]
            .iter()
            .enumerate()
        {
            rc.leafs.push(Leaf::new(array![*p].into(), 0.0, name, i));
        }
        rc.add_sum_product(&[vec![0, 1], vec![0, 2]], "a");
        rc.add_sum_product(&[vec![3, 0, 1], vec![3, 0, 2]], "b");
        let expected_a = 0.5 * (0.4 + 0.3);
        let expected_b = 0.8 * expected_a;

        let only_child = |rc: &TestRC, target: &str| {
            let children: Vec<NodeIndex> = rc
                .structure
                .neighbors_directed(rc.targets[target], Outgoing)
                .collect();
            assert_eq!(children.len(), 1, "{target} should have exactly one child");
            children[0]
        };
        let check_values = |rc: &mut TestRC| {
            for result in [rc.update(), rc.full_update()] {
                if let Some(a) = result.get("a") {
                    assert!(
                        (a[0] - expected_a).abs() < 1e-9,
                        "a: {} != {expected_a}",
                        a[0]
                    );
                }
                if let Some(b) = result.get("b") {
                    assert!(
                        (b[0] - expected_b).abs() < 1e-9,
                        "b: {} != {expected_b}",
                        b[0]
                    );
                }
            }
        };

        rc.lift_leaf(0, Topology::Dag);
        check_values(&mut rc);
        assert_ne!(only_child(&rc, "a"), only_child(&rc, "b"));

        rc.lift_leaf(3, Topology::Dag);
        check_values(&mut rc);
        assert_eq!(only_child(&rc, "a"), only_child(&rc, "b"));
        assert_eq!(rc.structure.node_count(), 3);

        rc.drop_leaf(0, Topology::Dag);
        check_values(&mut rc);
        assert_eq!(only_child(&rc, "a"), only_child(&rc, "b"));
        assert_eq!(rc.structure.node_count(), 3);
    }

    /// `a = x·y + x·z` merges into `b = w·(x·y + x·z)` after lifting `w`.
    /// Dropping `w` again must not multiply `w` into the node that holds `a`.
    #[test]
    fn test_drop_does_not_modify_target_node() {
        let mut rc = TestRC::new(1);
        for (i, (name, p)) in [("x", 0.5), ("y", 0.4), ("z", 0.3), ("w", 0.8)]
            .iter()
            .enumerate()
        {
            rc.leafs.push(Leaf::new(array![*p].into(), 0.0, name, i));
        }
        rc.add_sum_product(&[vec![0, 1], vec![0, 2]], "a");
        rc.add_sum_product(&[vec![3, 0, 1], vec![3, 0, 2]], "b");
        let expected_a = 0.5 * (0.4 + 0.3);
        let expected_b = 0.8 * expected_a;

        rc.lift_leaf(3, Topology::Dag);
        rc.drop_leaf(3, Topology::Dag);

        let result = rc.full_update();
        assert!(
            (result["a"][0] - expected_a).abs() < 1e-9,
            "a: {}",
            result["a"][0]
        );
        assert!(
            (result["b"][0] - expected_b).abs() < 1e-9,
            "b: {}",
            result["b"][0]
        );
    }

    /// Structure of the circuit below `node` as a string, looking through wrapper nodes.
    fn signature(rc: &TestRC, node: NodeIndex) -> String {
        let ac = &rc.structure[node];
        let child_of = |col: usize| {
            let edge = ac.col_memory_edge(col).unwrap();
            rc.structure.edge_endpoints(edge).unwrap().1
        };
        if ac.minterms.len() == 1
            && ac.minterms[0].len() == 1
            && ac.col_is_memory(ac.minterms[0][0])
        {
            return signature(rc, child_of(ac.minterms[0][0]));
        }
        let mut rows: Vec<String> = ac
            .minterms
            .iter()
            .map(|row| {
                let mut leaves: Vec<u32> = row
                    .iter()
                    .filter_map(|&c| match ac.columns[c] {
                        Column::Leaf(i) => Some(i),
                        Column::Memory(_) => None,
                    })
                    .collect();
                leaves.sort_unstable();
                let child = row
                    .iter()
                    .find(|&&c| ac.col_is_memory(c))
                    .map(|&c| signature(rc, child_of(c)))
                    .unwrap_or_default();
                format!("{leaves:?}{child}")
            })
            .collect();
        rows.sort_unstable();
        format!("({})", rows.join(" + "))
    }

    /// Reaching the same leaf levels by lifting only, dropping only, or a random
    /// mix of both yields the same structure and values.
    #[test]
    fn test_adaptation_is_path_independent() {
        use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};
        const VARIABLES: usize = 5;
        const BANDS: i32 = 3;

        for seed in 0..200u64 {
            let mut rng = StdRng::seed_from_u64(seed);
            // Variable v has leaves 2v (true) and 2v + 1 (false); rows are full worlds.
            let formulas: Vec<Vec<Vec<u32>>> = (0..2)
                .map(|_| {
                    (0..1u32 << VARIABLES)
                        .filter(|_| rng.random_bool(0.5))
                        .map(|world| {
                            (0..VARIABLES as u32)
                                .map(|v| 2 * v + ((world >> v) & 1))
                                .collect()
                        })
                        .collect()
                })
                .filter(|rows: &Vec<Vec<u32>>| !rows.is_empty())
                .collect();
            let levels: Vec<i32> = (0..VARIABLES).map(|_| rng.random_range(0..BANDS)).collect();
            let values: Vec<f64> = (0..VARIABLES).map(|_| rng.random_range(0.1..0.9)).collect();

            // Relative level `level + offset`: negative means lifts, positive drops.
            let operations = |offset: i32, rng: &mut StdRng| {
                let mut ops: Vec<(bool, u32)> = Vec::new();
                for (v, &level) in levels.iter().enumerate() {
                    let moves = level + offset;
                    for leaf in [2 * v as u32, 2 * v as u32 + 1] {
                        for _ in 0..moves.abs() {
                            ops.push((moves < 0, leaf));
                        }
                    }
                }
                ops.shuffle(rng);
                ops
            };
            let paths = [
                ("lift", operations(-(BANDS - 1), &mut rng)),
                ("drop", operations(0, &mut rng)),
                (
                    "mixed",
                    operations(-rng.random_range(1..BANDS - 1 + 1), &mut rng),
                ),
            ];

            for topology in [Topology::Tree, Topology::Dag] {
                let mut results: Vec<(&str, Vec<String>, Vec<f64>)> = Vec::new();
                for (name, ops) in &paths {
                    let mut rc = TestRC::new(1);
                    for (v, &p) in values.iter().enumerate() {
                        rc.leafs.push(Leaf::new(array![p].into(), 0.0, "", 2 * v));
                        rc.leafs
                            .push(Leaf::new(array![1.0 - p].into(), 0.0, "", 2 * v + 1));
                    }
                    for (t, formula) in formulas.iter().enumerate() {
                        rc.add_sum_product(formula, &format!("t{t}"));
                    }
                    for &(lift, leaf) in ops {
                        if lift {
                            rc.lift_leaf(leaf, topology);
                        } else {
                            rc.drop_leaf(leaf, topology);
                        }
                    }
                    let result = rc.full_update();
                    let targets = (0..formulas.len()).map(|t| format!("t{t}"));
                    let signatures = targets
                        .clone()
                        .map(|t| signature(&rc, rc.targets[&t]))
                        .collect();
                    let target_values = targets.map(|t| result[&t][0]).collect();
                    results.push((name, signatures, target_values));
                }

                let (reference, reference_signatures, reference_values) = &results[0];
                for (name, signatures, target_values) in &results[1..] {
                    for (a, b) in reference_values.iter().zip(target_values) {
                        assert!(
                            (a - b).abs() < 1e-9,
                            "seed {seed} {topology:?}: values differ"
                        );
                    }
                    assert_eq!(
                        reference_signatures, signatures,
                        "seed {seed} {topology:?}: {reference} vs {name}, levels {levels:?}"
                    );
                }
            }
        }
    }

    /// Dropping a leaf from many rows without children creates one shared node in DAG mode.
    #[test]
    fn test_drop_shares_leaf_node() {
        for (topology, expected_nodes) in [(Topology::Tree, 4), (Topology::Dag, 2)] {
            let mut rc = TestRC::new(1);
            for i in 0..4 {
                rc.leafs.push(Leaf::new(array![0.5].into(), 0.0, "", i));
            }
            rc.add_sum_product(&[vec![0, 1], vec![0, 2], vec![0, 3]], "t");
            let expected = rc.full_update()["t"][0];

            rc.drop_leaf(0, topology);

            assert_eq!(rc.structure.node_count(), expected_nodes, "{topology:?}");
            assert!((rc.full_update()["t"][0] - expected).abs() < 1e-9);
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

        rc.lift_leaf(0, Topology::Tree);

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
        reactive_circuit.lift_leaf(0, Topology::Tree);
        reactive_circuit.to_combined_svg("output/test/test_rc_lift_l0_rc.svg")?;
        assert_eq!(
            reactive_circuit
                .full_update()
                .get("test")
                .expect("The test target was not found in the RC!"),
            &value
        );

        reactive_circuit.drop_leaf(0, Topology::Tree);
        reactive_circuit.to_combined_svg("output/test/test_rc_lift_drop_l0_rc.svg")?;
        assert_eq!(
            reactive_circuit
                .full_update()
                .get("test")
                .expect("The test target was not found in the RC!"),
            &value
        );

        reactive_circuit.drop_leaf(0, Topology::Tree);
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
