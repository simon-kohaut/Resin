use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::File;
use std::io::Write;
use std::process::Command;

use itertools::Itertools;
use petgraph::{algo::toposort, stable_graph::{EdgeIndex, NodeIndex, StableGraph}, visit::EdgeRef};
use petgraph::Direction::{Incoming, Outgoing};

use crate::circuit::leaf::force_invalidate_dependencies;

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
        // Initialize ReactiveCircuit with a single AlgebraicCircuit inside
        let mut reactive_circuit = ReactiveCircuit::new(value_size);

        // Create single node and set as target
        let index = reactive_circuit
            .structure
            .add_node(AlgebraicCircuit::from_sum_product(value_size, sum_product));
        reactive_circuit.targets.insert(target_token, index);

        // Make leafs remember this node as dependency
        for leaf in reactive_circuit.leafs.iter_mut() {
            leaf.add_dependency(index.index() as u32);
        }

        // Queue up the node for recomputation
        reactive_circuit.queue.insert(index.index() as u32);

        reactive_circuit
    }

    pub fn new_target(&mut self, target_token: &str) -> NodeIndex {
        let node = self.structure.add_node(AlgebraicCircuit::new(self.value_size));
        self.targets.insert((*target_token).to_owned(), node);

        node
    }

    pub fn add_sum_product(&mut self, sum_product: &[Vec<u32>], target_token: &str) {
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
    }

    pub fn add(&mut self, product: &[u32], target_token: &str) {
        let target_node = self.targets[target_token];
        self.structure[target_node].add(product);

        for index in product {
            self.set_dependency(*index, &target_node);
        }

        self.queue.insert(target_node.index() as u32);
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
        loop {
            let mut active_memories = Vec::new();

            // Collect all mentioned memory nodes in algebraic circuits
            let mut nodes_to_remove = Vec::new();
            for node in self.structure.node_indices() {
                self.structure[node]
                    .get_scope(&self.structure[node].root)
                    .iter()
                    .for_each(|leaf| {
                        if let NodeType::Memory(index) = self.structure[node].structure[*leaf] {
                            active_memories.push(index);
                        }
                    });
            }

            // Check if any of the edges are not used anymore
            for edge in self.structure.edge_indices() {
                if !active_memories.contains(&edge) {
                    nodes_to_remove.push(self.structure.edge_endpoints(edge).unwrap().1);
                }
            }

            // Check if we are done
            if nodes_to_remove.is_empty() {
                break;
            }

            // Remove the nodes if necessary and repeat
            for node in nodes_to_remove {
                self.structure.remove_node(node);
            }
        }
    }

    /// Ensure that an AlgebraicCircuit with `index` within the ReactiveCircuit has a parent, e.g., to lift a leaf into.
    fn ensure_parent(&mut self, index: u32) -> Vec<(NodeIndex, EdgeIndex)> {
        let parents_and_edges: Vec<(NodeIndex, EdgeIndex)> = self
            .structure
            .edges_directed(index.into(), Incoming)
            .map(|edge| (edge.source(), edge.id()))
            .collect();

        if parents_and_edges.is_empty() {
            // Create the missing parent and its edge potining at the specified circuit
            let parent = self
                .structure
                .add_node(AlgebraicCircuit::new(self.value_size));
            let edge = self
                .structure
                .add_edge(parent, index.into(), Vector::ones(self.value_size));

            // Note that this new parent is invalid
            self.queue.insert(parent.index() as u32);

            // Access the parent mutably
            let algebraic_circuit = self
                .structure
                .node_weight_mut(parent)
                .expect("Leaf dependency was not found in Reactive Circuit!");

            // Add a memory node pointing at the new edge to the circuit
            let memory_index = algebraic_circuit.create_memory(edge);
            algebraic_circuit.add_to_nodes(&vec![algebraic_circuit.root], &vec![memory_index]);

            // Update targets if this node was one before
            let tokens_to_update: Vec<String> = self
                .targets
                .iter()
                .filter(|(_, &node_index)| node_index.index() == index as usize)
                .map(|(token, _)| token.clone())
                .collect();

            for token in tokens_to_update {
                self.targets.insert(token, parent);
            }

            return vec![(parent, edge)];
        }

        return parents_and_edges;
    }

    /// Lift the leaf with `index` out of its current circuits into its ancestors.
    pub fn lift_leaf(&mut self, index: u32) {
        let dependencies = self.leafs[index as usize].get_dependencies().clone();
        for dependency in dependencies {
            let dependency_node: NodeIndex = dependency.into();
            let ac = self.structure.node_weight_mut(dependency_node).unwrap();
            let leaf_node = match ac.get_leaf(index) {
                Some(node) => node,
                None => continue, // Leaf not in this circuit, must be an ancestor dependency.
            };

            let parents_and_edges = self.ensure_parent(dependency);
            let ac = self.structure.node_weight_mut(dependency_node).unwrap();
            let (in_scope_circuit, out_of_scope_circuit) = ac.split(&leaf_node);

            if let Some(circuit) = out_of_scope_circuit {
                self.handle_out_of_scope_circuit(circuit, &parents_and_edges);
            }

            if let Some(circuit) = in_scope_circuit {
                self.handle_in_scope_circuit(
                    circuit,
                    dependency,
                    dependency_node,
                    &leaf_node,
                    index,
                    &parents_and_edges,
                );
            }
        }

        force_invalidate_dependencies(self, index);
    }

    fn handle_out_of_scope_circuit(
        &mut self,
        circuit: AlgebraicCircuit,
        parents_and_edges: &[(NodeIndex, EdgeIndex)],
    ) {
        let out_of_scope_node = self.structure.add_node(circuit);
        for (parent, in_scope_edge) in parents_and_edges {
            let out_of_scope_edge = self
                .structure
                .add_edge(*parent, out_of_scope_node, Vector::ones(self.value_size));
            let parent_ac = self.structure.node_weight_mut(*parent).unwrap();

            let in_scope_memory = parent_ac.get_memory(*in_scope_edge).unwrap();
            let out_of_scope_memory = parent_ac.create_memory(out_of_scope_edge);

            let mut product = parent_ac.get_siblings(&in_scope_memory);
            product.push(out_of_scope_memory);
            parent_ac.add_to_nodes(&parent_ac.get_parents(&in_scope_memory), &product);
        }
    }

    fn handle_in_scope_circuit(
        &mut self,
        circuit: AlgebraicCircuit,
        dependency: u32,
        dependency_node: NodeIndex,
        leaf_node: &NodeIndex,
        leaf_index: u32,
        parents_and_edges: &[(NodeIndex, EdgeIndex)],
    ) {
        *self.structure.node_weight_mut(dependency_node).unwrap() = circuit;
        let ac = self.structure.node_weight_mut(dependency_node).unwrap();
        ac.remove(leaf_node);

        self.leafs[leaf_index as usize].remove_dependency(dependency);
        self.queue.insert(dependency);

        for (parent, edge) in parents_and_edges {
            let parent_ac = &mut self.structure[*parent];
            let parent_leaf = parent_ac.ensure_leaf(leaf_index);
            let memory = parent_ac.get_memory(*edge).unwrap(); // Old edge with same numeric index but fails
            parent_ac.multiply_with_nodes(&vec![memory], &vec![parent_leaf]);
            self.leafs[leaf_index as usize].add_dependency(parent.index() as u32);
        }
    }

    /// Drop the leaf with `index` out of its current circuits into its ancestors.
    pub fn drop_leaf(&mut self, index: u32) {
        let dependencies = self.leafs[index as usize].get_dependencies().clone();
        for dependency in dependencies {
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
        }

        force_invalidate_dependencies(self, index);
    }

    fn handle_leaf_drop_for_product(&mut self, leaf_index: u32, dependency: NodeIndex, product: NodeIndex) {
        let memory_sibling = self.structure[dependency]
            .iter_children(&product)
            .find(|&child| self.structure[dependency].check_node_type(&child, &NodeType::Memory(EdgeIndex::default())));

        if let Some(memory_node) = memory_sibling {
            if let NodeType::Memory(edge) = self.structure[dependency].structure[memory_node] {
                let (_, child) = self.structure.edge_endpoints(edge).unwrap();
                self.structure[child].multiply(leaf_index);
                self.leafs[leaf_index as usize].add_dependency(child.index() as u32);
            }
        } else {
            let new_ac = AlgebraicCircuit::from_sum_product(self.value_size, &vec![vec![leaf_index]]);
            let new_node = self.structure.add_node(new_ac);
            let new_edge = self.structure.add_edge(dependency, new_node, Vector::ones(self.value_size));

            let ac = self.structure.node_weight_mut(dependency).unwrap();
            let new_memory_node = ac.structure.add_node(NodeType::Memory(new_edge));
            ac.structure.add_edge(product, new_memory_node, ());

            self.leafs[leaf_index as usize].add_dependency(new_node.index() as u32);
        }
    }

    /// Update the necessary values within the ReactiveCircuit and its output.
    /// Returns a `HashMap<String, Vector>` where the key is a target token and the value
    /// contains the computed outcome.
    pub fn update(&mut self) -> HashMap<String, Vector> {
        // We collect data to share to the outside world
        let mut target_results = HashMap::new();

        // For each outdated circuit, we recompute the memorized value as edge weight
        let mut sorted_nodes = toposort(&self.structure, None).expect("ReactiveCircuit should be a DAG");
        while let Some(outdated_algebraic_circuit) = sorted_nodes.pop() {
            // Check if that circuit is invalid and if not continue on
            if !self.queue.contains(&(outdated_algebraic_circuit.index() as u32)) {
                continue;
            } else {
                self.queue.remove(&(outdated_algebraic_circuit.index() as u32));
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
                    "P({}) = ΣΠ\\n{} - N{} - E{}",
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
                                format!("M{:?}", index)
                            } else {
                                unreachable!()
                            }
                        })
                        .collect::<Vec<String>>()
                        .join(" "),
                    self.structure[node].structure.node_count(),
                    self.structure[node].structure.edge_count()
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
                "    {} -> {} [label=\"M{}\" decorate=\"true\"];\n",
                source.index(),
                target.index(),
                edge.index()
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
        let number_leafs = 10;
        let number_products = 20;
        let product_size = 3;
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
        for step in 0..simulation_steps {
            // Calculate expected value before any changes in this step
            let leaf_values = reactive_circuit
                .leafs
                .iter()
                .map(|l| l.get_value())
                .collect::<Vec<_>>();
            let expected_value = calculate_expected_value(&sum_of_products, &leaf_values, value_size);

            // Check current value
            let result = reactive_circuit.full_update();
            println!("{} | {}", result["random_target"].clone(), expected_value.clone());
            assert!((result["random_target"].clone() - expected_value.clone()).sum().abs() < 1e-9);

            // Randomly update a leaf
            if rng.random_bool(0.5) {
                let leaf_to_update = rng.random_range(0..number_leafs) as u32;
                let new_value = Vector::from(vec![rng.random_range(0.0..1.0)]);
                update(&mut reactive_circuit, leaf_to_update, new_value, step as f64);
            }

            // Randomly adapt structure
            reactive_circuit.to_combined_svg("test_randomized_rc_after.svg");
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
        println!("{:?}", reactive_circuit.queue);
        reactive_circuit.lift_leaf(0);
        println!("{:?}", reactive_circuit.queue);
        reactive_circuit.to_combined_svg("output/test/test_rc_lift_l0_rc.svg")?;
        assert_eq!(reactive_circuit.update().get("test").unwrap(), &value);
        assert_eq!(reactive_circuit.full_update().get("test").expect("The test target was not found in the RC!"), &value);

        reactive_circuit.drop_leaf(0);
        reactive_circuit.to_combined_svg("output/test/test_rc_lift_drop_l0_rc.svg")?;
        assert_eq!(reactive_circuit.update().get("test").unwrap(), &value);
        assert_eq!(reactive_circuit.full_update().get("test").expect("The test target was not found in the RC!"), &value);
        
        reactive_circuit.drop_leaf(0);
        reactive_circuit.to_combined_svg("output/test/test_rc_lift_drop_drop_l0_rc.svg")?;
        assert_eq!(reactive_circuit.update().get("test").unwrap(), &value);
        assert_eq!(reactive_circuit.full_update().get("test").expect("The test target was not found in the RC!"), &value);

        Ok(())
    }
}
