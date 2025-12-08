use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::Write;
use std::process::Command;

use clingo::ast::Edge;
use itertools::Itertools;
use linfa::linalg::assert;
use petgraph::{algo::toposort, stable_graph::{EdgeIndex, NodeIndex, StableGraph}, visit::EdgeRef};
use petgraph::Direction::{Incoming, Outgoing};
use plotly::sankey::Node;
use rayon::in_place_scope;

use crate::circuit::leaf::{self, force_invalidate_dependencies};

use super::{algebraic::AlgebraicCircuit, algebraic::NodeType, leaf::Leaf, Vector};

/// A dynamic computation graph where each node contains an `AlgebraicCircuit` for which the result is
/// stored as weight of the incoming edges.
///
/// A ReactiveCircuit owns its `structure` as a `StableGraph<AlgebraicCircuit, Vector>`, where each
/// `Vector` is of length `value_size`.
///
/// Further, it has `leafs` and a `queue`, with the former holding time-dynamic input data and
/// the latter holding indices to the `AlgebraicCircuits` that need reevaluation due to an update
/// of a contained `leaf` or one of its decendants.
#[derive(Debug, Clone)]
pub struct ReactiveCircuit {
    pub structure: StableGraph<AlgebraicCircuit, Vector>,
    pub value_size: usize,
    pub leafs: Vec<Leaf>,
    pub queue: HashSet<u32>,
    pub targets: HashMap<String, NodeIndex>,
}

impl ReactiveCircuit {
    /// Create a new `ReactiveCircuit` with the given `value_size` and set of `leafs`.
    pub fn new(value_size: usize) -> Self {
        assert!(value_size > 0, "value_size needs to be positive integer greater than 0!");
        
        ReactiveCircuit {
            structure: StableGraph::new(),
            value_size,
            leafs: Vec::new(),
            queue: HashSet::new(),
            targets: HashMap::new(),
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

    pub fn new_target(&mut self, target_token: &str) -> NodeIndex {
        // TODO: Using this function leaves the RC in a bad state (empty AC node)
        // Maybe remove method or require formula?
        assert!(!self.targets.contains_key(target_token), "Cannot add multiple targets with the same name!");

        let node = self.structure.add_node(AlgebraicCircuit::new(self.value_size));
        self.targets.insert((*target_token).to_owned(), node);

        node
    }

    pub fn add_sum_product(&mut self, sum_product: &[Vec<u32>], target_token: &str) {
        self.check_invariants();

        if !self.targets.contains_key(target_token) {
            self.targets.insert(target_token.to_string(), self.structure.add_node(AlgebraicCircuit::new(self.value_size)));
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

    pub fn set_dependency(&mut self, index: u32, node: &NodeIndex) {
        self.leafs[index as usize].add_dependency(node.index() as u32);
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

    pub fn invalidate(&mut self) {
        // Invalidate in a bottom-up fashion so that the update queue can be processed from bottom to top
        let sorted_nodes = toposort(&self.structure, None).expect("ReactiveCircuit should be a DAG");
        self.queue.extend(sorted_nodes.iter().map(|node| node.index() as u32));
        self.queue = self.queue.iter().unique().cloned().collect();
    }

    pub fn prune(&mut self) {
        // Collect nodes that seem safe to remove
        let mut nodes_to_remove = Vec::new();
        for node in self.structure.node_indices() {
            if self.structure[node].leafs.is_empty() && self.structure[node].memories.is_empty() {
                nodes_to_remove.push(node);
            }
        }
    
        // For each of these nodes, we need to ensure no other node holds a memory of it.
        for node_to_remove in nodes_to_remove {
            if !self.structure.contains_node(node_to_remove) {
                continue;
            }
            
            let mut incident_edges: Vec<EdgeIndex> = self.structure.edges_directed(node_to_remove, Incoming).map(|e| e.id()).collect();
            incident_edges.extend(self.structure.edges_directed(node_to_remove, Outgoing).map(|e| e.id()));
    
            // Collect node indices to avoid borrowing issues while modifying node weights.
            let all_node_indices: Vec<NodeIndex> = self.structure.node_indices().collect();

            // Remove any memory nodes in other algebraic circuits that point to this node
            for node_idx in all_node_indices {
                if node_idx == node_to_remove {
                    continue;
                }
                let ac = self.structure.node_weight_mut(node_idx).unwrap();
                for edge in &incident_edges {
                    if let Some(mem_node) = ac.get_memory(*edge) {
                        ac.remove(&mem_node);
                    }
                }
            }
    
            // Now it is safe to remove the node
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
            // Create the missing parent and its edge potining at the specified circuit
            let parent = self
                .structure
                .add_node(AlgebraicCircuit::new(self.value_size));
            let edge = self
                .structure
                .add_edge(parent, index, Vector::ones(self.value_size));

            // Note that this new parent is invalid
            self.queue.insert(parent.index() as u32);

            // Access the parent mutably
            let algebraic_circuit = self.structure.node_weight_mut(parent).unwrap();

            // Add a memory node pointing at the new edge to the circuit
            let memory_index = algebraic_circuit.create_memory(edge);
            algebraic_circuit.add_to_nodes(&vec![algebraic_circuit.root], &vec![memory_index]);

            // Update targets if this node was one before
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

        return parents_and_edges;
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
        for dependency in self.leafs[index as usize].get_dependencies() {
            self.check_invariants();

            // Check if this node actually contains leaf
            if self.structure.node_weight(dependency.into()).unwrap().get_leaf(index).is_none() {
                continue;
            }

            // Get mutable access to dependency with leaf and ensure they have at least one parent node to lift leaf into
            let node_to_lift = dependency.into();
            let parents_and_edges = self.ensure_parent(node_to_lift);
            let ac = self.structure.node_weight_mut(node_to_lift).unwrap();

            // Split into part that has leaf and part without that leaf
            let (in_scope_circuit, out_of_scope_circuit) = ac.split(index);

            let out_of_scope_node = match out_of_scope_circuit {
                Some(circuit) => {
                    let node = self.structure.add_node(circuit);
                    let memories = self.structure.node_weight_mut(node).unwrap().memories.clone();
                    for (edge, memory_node) in memories {
                        let old_edge_weight = self.structure.edge_weight(edge.into()).unwrap();
                        let old_edge_target = self.structure.edge_endpoints(edge.into()).unwrap().1;
                        
                        let new_edge = self.structure.add_edge(node, old_edge_target, old_edge_weight.clone()).index() as u32;
                        
                        self.structure.node_weight_mut(node).unwrap().memories.remove(&edge);
                        self.structure.node_weight_mut(node).unwrap().memories.insert(new_edge, memory_node);
                        self.structure.node_weight_mut(node).unwrap().structure[memory_node] = NodeType::Memory(new_edge.into());
                    }

                    Some(node)
                }
                None => None
            };

            let in_scope_node = match in_scope_circuit {
                Some(mut circuit) => {
                    circuit.remove(&circuit.get_leaf(index).unwrap());

                    let node = self.structure.add_node(circuit);
                    let memories = self.structure.node_weight_mut(node).unwrap().memories.clone();
                    for (edge, memory_node) in memories {
                        let old_edge_weight = self.structure.edge_weight(edge.into()).unwrap();
                        let old_edge_target = self.structure.edge_endpoints(edge.into()).unwrap().1;
                        
                        let new_edge = self.structure.add_edge(node, old_edge_target, old_edge_weight.clone()).index() as u32;

                        self.structure.node_weight_mut(node).unwrap().memories.remove(&edge);
                        self.structure.node_weight_mut(node).unwrap().memories.insert(new_edge, memory_node);
                        self.structure.node_weight_mut(node).unwrap().structure[memory_node] = NodeType::Memory(new_edge.into());
                    }

                    node
                },
                None => unreachable!()
            };

            // Removes the lifted leaf and creates a new node with this circuit
            for (parent, edge) in parents_and_edges {
                let original_product = self.disconnect(parent, node_to_lift);
                let mut factors = self.structure.node_weight_mut(parent).unwrap().get_children(&original_product);

                let leaf = self.structure.node_weight_mut(parent).unwrap().ensure_leaf(index);
                factors.push(leaf);
                let in_scope_product = self.structure.node_weight_mut(parent).unwrap().add_to_node(&original_product, &factors);

                if self.structure.node_weight_mut(in_scope_node).unwrap().structure.node_indices().count() == 2 {
                    self.structure.remove_node(in_scope_node);
                } else {
                    self.connect(parent, in_scope_node, in_scope_product);
                    self.queue.insert(in_scope_node.index() as u32);
                }

                if out_of_scope_node.is_some() {
                    self.connect(parent, out_of_scope_node.unwrap(), original_product);
                    self.queue.insert(out_of_scope_node.unwrap().index() as u32);
                } else {
                    self.structure.node_weight_mut(parent).unwrap().remove(&original_product);
                }
            }

            // Remove lifted node from dependencies
            self.structure.remove_node(node_to_lift);
        }

        self.update_dependencies();

        self.check_invariants();
    }

    /// Drop the leaf with `index` out of its current circuits into its ancestors.
    pub fn drop_leaf(&mut self, index: u32) {
        self.check_invariants();

        for dependency in self.leafs[index as usize].get_dependencies() {
            let dependency: NodeIndex = dependency.into();
            let leaf_node = match self.structure[dependency].get_leaf(index) {
                Some(node) => node,
                None => continue, // Leaf not in this circuit, must be an ancestor dependency.
            };

            let products = self.structure[dependency].get_parents(&leaf_node);
            for product in products {
                self.handle_leaf_drop_for_product(index, dependency, product);
            }

            self.structure.node_weight_mut(dependency).unwrap().remove(&leaf_node);
            
            for child in self.structure.neighbors_directed(dependency, Outgoing) {
                self.queue.insert(child.index() as u32);
            }
        }

        self.update_dependencies();

        self.check_invariants();
    }

    fn handle_leaf_drop_for_product(&mut self, leaf_index: u32, dependency: NodeIndex, product: NodeIndex) {
        let memory_sibling = self.structure[dependency]
            .iter_children(&product)
            .find(|&child| self.structure[dependency].check_node_type(&child, &NodeType::Memory(EdgeIndex::default())));

        if let Some(memory_node) = memory_sibling {
            if let NodeType::Memory(edge) = self.structure[dependency].structure[memory_node] {
                let (_, child) = self.structure.edge_endpoints(edge).unwrap();
                self.structure[child].multiply(leaf_index);
            } else {
                unreachable!()
            }
        } else {
            let new_ac = AlgebraicCircuit::from_sum_product(self.value_size, &vec![vec![leaf_index]]);
            let new_node = self.structure.add_node(new_ac);
            let new_edge = self.structure.add_edge(dependency, new_node, Vector::ones(self.value_size));

            let ac = self.structure.node_weight_mut(dependency).unwrap();
            let new_memory_node = ac.create_memory(new_edge);
            ac.structure.add_edge(product, new_memory_node, ());
        }
    }

    /// Create a memory in the parent node's product as well as a new edge to the given child node.
    pub fn connect(&mut self, parent: NodeIndex, child: NodeIndex, product: NodeIndex) -> NodeIndex {
        let edge: EdgeIndex = self.structure.add_edge(parent, child, Vector::ones(self.value_size));
        let memory: NodeIndex = self.structure.node_weight_mut(parent).unwrap().create_memory(edge);
        self.structure.node_weight_mut(parent).unwrap().multiply_with_nodes(&vec![product], &vec![memory]);

        memory
    }

    /// Disconnects a parent node from its child by removing the edge and corresponding memory node.
    pub fn disconnect(&mut self, parent: NodeIndex, child: NodeIndex) -> NodeIndex {
        let edges: Vec<_> = self.structure.edges_connecting(parent, child)
            .map(|edge_ref| edge_ref.id())
            .collect();
        let edge: EdgeIndex = edges[0];
        let memory: NodeIndex = self.structure.node_weight_mut(parent).unwrap().get_memory(edge).unwrap();
        let product: NodeIndex = self.structure.node_weight_mut(parent).unwrap().get_parents(&memory)[0];
        self.structure.node_weight_mut(parent).unwrap().remove(&memory);
        self.structure.remove_edge(edge);

        product
    }

    /// Update the necessary values within the ReactiveCircuit and its output.
    /// Returns a `HashMap<String, Vector>` where the key is a target token and the value
    /// contains the computed outcome.
    pub fn update(&mut self) -> HashMap<String, Vector> {
        // We collect data to share to the outside world
        let mut target_results = HashMap::new();
        let outdated_nodes = self.queue.clone();
        self.queue.clear();

        // For each outdated circuit, we recompute the memorized value as edge weight
        let mut sorted_nodes = toposort(&self.structure, None).expect("ReactiveCircuit should be a DAG");
        while let Some(outdated_algebraic_circuit) = sorted_nodes.pop() {
            if !outdated_nodes.contains(&(outdated_algebraic_circuit.index() as u32)) {
                continue;
            }

            // Get the new value of the AlgebraicCircuit
            let result = self
                .structure
                .node_weight(outdated_algebraic_circuit.into())
                .expect("AlgebraicCircuit was missing!")
                .value(self);

            // If this dependency is a target, add to results
            for (token, index) in self.targets.iter() {
                if index.index() == outdated_algebraic_circuit.index() {
                    target_results.insert(token.to_owned(), result.clone());
                }
            }

            // Memorize the result in all incoming edges
            let edges: Vec<EdgeIndex> = self
                .structure
                .edges_directed(outdated_algebraic_circuit.into(), Incoming)
                .map(|e| e.id())
                .collect();
            for edge in edges.iter() {
                self.structure
                    .edge_weight_mut(*edge)
                    .expect("ReactiveCircuit edge was missing!")
                    .assign(&result);
            }
        }

        target_results
    }

    // Full update of the Reactive Circuit independent of the current queue, but emptying the queue afterwards
    pub fn full_update(&mut self) -> HashMap<String, Vector> {
        self.invalidate();
        self.update()
    }

    #[cfg(debug_assertions)]
    pub fn check_invariants(&self) {
        let mut violations = Vec::new();

        // Invariant 1: For every edge that exists in the reactive circuit, the source node's
        // algebraic circuit must have a corresponding memory node.
        for edge_index in self.structure.edge_indices() {
            let (source, target) = self.structure.edge_endpoints(edge_index).unwrap();
            let source_ac = &self.structure[source];
            if source_ac.get_memory(edge_index).is_none() {
                violations.push(format!(
                    "Invariant Violation: Edge {:?} from {:?} to {:?} exists, but source AC is missing memory node.",
                    edge_index,
                    source,
                    target
                ));
            }
        }

        // Invariant 2: For every memory node in an algebraic circuit, the edge it refers to
        // must exist in the reactive circuit.
        for node_index in self.structure.node_indices() {
            let ac = &self.structure[node_index];
            for edge_index_u32 in ac.memories.keys() {
                let edge_index = EdgeIndex::new(*edge_index_u32 as usize);
                if self.structure.edge_weight(edge_index).is_none() {
                    violations.push(format!(
                        "Invariant Violation: Node {:?} has memory of edge {:?}, but this edge does not exist.",
                        node_index,
                        edge_index
                    ));
                }
            }
        }

        // Invariant 3: Every node has a non-empty algebraic circuit (beyond a sum and a product node).
        for node_index in self.structure.node_indices() {
            if self.structure[node_index].structure.node_indices().count() <= 2 {
                violations.push(format!(
                    "Invariant Violation: Node {:?} has an empty algebraic circuit.",
                    node_index
                ));
            }
        }

        // Invariant 4: Every node has a non-empty scope.
        for node_index in self.structure.node_indices() {
            if self.structure[node_index].get_scope(&self.structure[node_index].root).is_empty() {
                violations.push(format!(
                    "Invariant Violation: Node {:?} has an empty scope.",
                    node_index
                ));
            }
        }

        if !violations.is_empty() {
            self.to_svg("invariant_violation.svg", true);
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
            let node_type = &self.structure[node];
            let node_label = match node_type {
                algebraic_circuit => format!(
                    "P({}) = ΣΠ\\n{}",
                    // "P({}) = ΣΠ\\n{}\\n - N{} - E{}\\nLeafs {:?}\\nMemory{:?}",
                    self.targets
                        .iter()
                        .filter(|(_, v)| **v == node)
                        .map(|(k, _)| k)
                        .join(""),
                    algebraic_circuit
                        .get_scope(&algebraic_circuit.root)
                        .iter()
                        .map(|leaf| {
                            if let NodeType::Leaf(index) = algebraic_circuit.structure[*leaf] {
                                format!("L{}", index)
                            } else if let NodeType::Memory(index) = algebraic_circuit.structure[*leaf] {
                                format!("M{:?}", index.index())
                            } else {
                                unreachable!()
                            }
                        })
                        .collect::<Vec<String>>()
                        .join(" "),
                    // self.structure[node].structure.node_count(),
                    // self.structure[node].structure.edge_count(),
                    // self.structure[node].leafs,
                    // self.structure[node].memories
                ),
            };
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
                "    {} -> {} [label=\"M{}={}\" decorate=\"true\"];\n",
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
        file.write_all(&self.to_dot_text().as_bytes())?;

        // Gather dot text for all contained AlgebraicCircuits
        for node in self.structure.node_indices() {
            file.write_all(&self.structure[node].to_dot_text().as_bytes())?;
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

    use rand::prelude::IndexedRandom;
    use rand::Rng;

    use super::*;
    use std::collections::BTreeSet;

    use crate::channels::manager::Manager;
    use crate::circuit::leaf::update;

    use super::Vector;

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
        let manager = Manager::new(value_size);
        let mut reactive_circuit = manager.reactive_circuit.lock().unwrap();

        // 2. Create a large, random formula
        for i in 0..number_leafs {
            reactive_circuit.leafs.push(Leaf::new(
                Vector::from(vec![rng.random_range(0.0..1.0)]),
                0.0,
                &format!("leaf_{}", i),
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
        reactive_circuit.to_svg("test_randomized_rc.svg", false);

        // 3. Simulation loop
        for step in 0..simulation_steps+1 {
            // Calculate expected value before any changes in this step
            let leaf_values = reactive_circuit
                .leafs
                .iter()
                .map(|l| l.get_value())
                .collect::<Vec<_>>();
            let expected_value = calculate_expected_value(&sum_of_products, &leaf_values, value_size);

            // Check if full update results in expected value
            let result = reactive_circuit.full_update();
            println!("RC result = {} | Expected = {}", result["random_target"].clone(), expected_value.clone());
            assert!((result["random_target"].clone() - expected_value.clone()).sum().abs() < 1e-9);

            // Randomly update a leaf
            if rng.random_bool(0.5) {
                let leaf_to_update = rng.random_range(0..number_leafs) as u32;
                let new_value = Vector::from(vec![rng.random_range(0.0..1.0)]);
                update(&mut reactive_circuit, leaf_to_update, new_value, step as f64);
            }

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
    fn test_rc() -> std::io::Result<()> {
        let manager = Manager::new(1);
        let reactive_circuit = &mut manager.reactive_circuit.lock().unwrap();
        
        reactive_circuit.leafs.push(Leaf::new(Vector::ones(1), 0.0, ""));
        reactive_circuit.leafs.push(Leaf::new(Vector::ones(1), 0.0, ""));
        reactive_circuit.leafs.push(Leaf::new(Vector::ones(1), 0.0, ""));
        
        reactive_circuit.add_sum_product(&vec![vec![0, 1], vec![0, 2]], "test");

        assert_eq!(reactive_circuit.leafs.len(), 3);
        assert_eq!(reactive_circuit.structure.node_count(), 1);
        assert_eq!(reactive_circuit.structure.node_weight(0.into()).unwrap().structure.node_count(), 6);
        assert!(reactive_circuit.leafs.iter().all(|leaf| leaf.get_dependencies().len() == 1));
        assert!(reactive_circuit.leafs.iter().all(|leaf| leaf.get_dependencies() == BTreeSet::from_iter(vec![0])));

        let results = reactive_circuit.update();
        let value = results.get("test").expect("The key 'test' was not found in the results").clone();
        reactive_circuit.to_combined_svg("output/test/test_rc_original.svg")?;

        // Structural changes require updates
        // Partial and full updates always gives the same result
        reactive_circuit.lift_leaf(0);
        reactive_circuit.to_combined_svg("output/test/test_rc_lift_l0_rc.svg")?;
        assert_eq!(reactive_circuit.full_update().get("test").expect("The test target was not found in the RC!"), &value);

        reactive_circuit.drop_leaf(0);
        reactive_circuit.to_combined_svg("output/test/test_rc_lift_drop_l0_rc.svg")?;
        assert_eq!(reactive_circuit.full_update().get("test").expect("The test target was not found in the RC!"), &value);
        
        reactive_circuit.drop_leaf(0);
        reactive_circuit.to_combined_svg("output/test/test_rc_lift_drop_drop_l0_rc.svg")?;
        assert_eq!(reactive_circuit.full_update().get("test").expect("The test target was not found in the RC!"), &value);

        Ok(())
    }
}
