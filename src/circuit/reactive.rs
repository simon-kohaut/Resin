use std::collections::BTreeSet;
use std::fs::File;
use std::io::Write;
use std::process::Command;
use std::sync::{Arc, Mutex};

use petgraph::stable_graph::{EdgeIndex, NodeIndex, StableGraph};
use petgraph::visit::{EdgeRef, NodeRef};
use petgraph::Direction::{Incoming, Outgoing};

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
#[derive(Debug)]
pub struct ReactiveCircuit {
    pub structure: StableGraph<AlgebraicCircuit, Vector>,
    pub value_size: usize,
    pub leafs: Vec<Leaf>,
    pub queue: BTreeSet<u32>,
}

impl ReactiveCircuit {
    /// Create a new `ReactiveCircuit` with the given `value_size` and set of `leafs`.
    pub fn new(value_size: usize, leafs: &[Leaf]) -> Self {
        ReactiveCircuit {
            structure: StableGraph::new(),
            value_size,
            leafs: leafs.into(),
            queue: BTreeSet::new(),
        }
    }

    /// Initialize the ReactiveCircuit from a single sum-product formula.
    pub fn from_sum_product(value_size: usize, leafs: &[Leaf], sum_product: &[Vec<u32>]) -> Self {
        // Initialize ReactiveCircuit with a single AlgebraicCircuit inside
        let mut reactive_circuit = ReactiveCircuit::new(value_size, leafs);
        let index = reactive_circuit
            .structure
            .add_node(AlgebraicCircuit::from_sum_product(value_size, sum_product));

        for leaf in reactive_circuit.leafs.iter_mut() {
            leaf.add_dependency(index.index() as u32);
        }

        reactive_circuit
    }

    /// Ensure that an AlgebraicCircuit with `index` within the ReactiveCircuit has a parent, e.g., to lift a leaf into.
    fn ensure_parent(&mut self, index: u32) -> Vec<(NodeIndex, EdgeIndex)> {
        let parents_and_edges: Vec<(NodeIndex, EdgeIndex)> = self
            .structure
            .edges_directed(index.into(), Incoming)
            .map(|edge| (edge.source().id(), edge.id()))
            .collect();

        if parents_and_edges.is_empty() {
            // Create the missing parent and its edge potining at the specified circuit
            let parent = self
                .structure
                .add_node(AlgebraicCircuit::new(self.value_size));
            let edge = self
                .structure
                .add_edge(parent, index.into(), Vector::ones(self.value_size));

            // Access the parent mutably
            let algebraic_circuit = self
                .structure
                .node_weight_mut(parent)
                .expect("Leaf dependency was not found in Reactive Circuit!");

            // Add a memory node pointing at the new edge to the circuit
            let memory_index = algebraic_circuit.structure.add_node(NodeType::Memory(edge));
            algebraic_circuit.add_to_nodes(&vec![algebraic_circuit.root], &vec![memory_index]);

            return vec![(parent, edge)];
        }

        return parents_and_edges;
    }

    /// Ensure that an AlgebraicCircuit with `index` within the ReactiveCircuit has a child, e.g., to drop a leaf into.
    fn ensure_child(&mut self, index: u32) -> Vec<(NodeIndex, EdgeIndex)> {
        let children_and_edges: Vec<(NodeIndex, EdgeIndex)> = self
            .structure
            .edges_directed(index.into(), Outgoing)
            .map(|edge| (edge.target().id(), edge.id()))
            .collect();

        if children_and_edges.is_empty() {
            // Create the missing parent and its edge potining at the specified circuit
            let child = self
                .structure
                .add_node(AlgebraicCircuit::new(self.value_size));
            let edge = self
                .structure
                .add_edge(child, index.into(), Vector::ones(self.value_size));

            // Access the parent mutably
            let algebraic_circuit = self
                .structure
                .node_weight_mut(child)
                .expect("Leaf dependency was not found in Reactive Circuit!");

            // Add a memory node pointing at the new edge to the circuit
            let memory_index = algebraic_circuit.structure.add_node(NodeType::Memory(edge));
            algebraic_circuit.add_to_nodes(&vec![algebraic_circuit.root], &vec![memory_index]);

            return vec![(child, edge)];
        }

        return children_and_edges;
    }

    /// Lift the leaf with `index` out of its current circuits into its ancestors.
    pub fn lift(&mut self, index: u32) {
        // Get all the circuits that depend on the leaf
        let dependencies = self.leafs[index as usize].get_dependencies();

        for dependency in dependencies {
            // If leaf is not a node within the circuit, go to next dependency
            // (i.e., this is just an ancestor dependency)
            let leaf_node;
            match self
                .structure
                .node_weight(dependency.into())
                .expect("Dependency was missing!")
                .get_leaf(index)
            {
                Some(node) => leaf_node = node,
                None => continue,
            }

            // Ensure that the dependant has at least one parent, else we have no circuit to lift into
            let parents_and_edges = self.ensure_parent(dependency);

            // Setup optional new circuits to contain part of algebra that has the leaf in scope and a part that has not
            let (in_scope_circuit, out_of_scope_circuit) = self
                .structure
                .node_weight_mut(dependency.into())
                .expect("Dependency was missing!")
                .split(&leaf_node);

            if out_of_scope_circuit.is_some() {
                // Create a new node in the Reactive Circuit
                let out_of_scope_circuit_node =
                    self.structure.add_node(out_of_scope_circuit.unwrap());

                // Add the old product with new memory node as new sum to the parents
                for (parent, in_scope_edge) in &parents_and_edges {
                    let out_of_scope_edge = self.structure.add_edge(
                        *parent,
                        out_of_scope_circuit_node,
                        Vector::ones(self.value_size),
                    );
                    let parent_node = self
                        .structure
                        .node_weight_mut(*parent)
                        .expect("Parent node was missing!");

                    let in_scope_memory = parent_node
                        .get_memory(*in_scope_edge)
                        .expect("Memory node was missing!");
                    let out_of_scope_memory = parent_node
                        .structure
                        .add_node(NodeType::Memory(out_of_scope_edge));

                    let mut product = parent_node.get_siblings(&in_scope_memory);
                    product.push(out_of_scope_memory);

                    parent_node.add_to_nodes(&parent_node.get_parents(&in_scope_memory), &product);
                }
            }

            if in_scope_circuit.is_some() {
                // We override the old circuit with the in-scope one and remove the leaf
                *self.structure.node_weight_mut(dependency.into()).unwrap() =
                    in_scope_circuit.unwrap();
                self.structure
                    .node_weight_mut(dependency.into())
                    .unwrap()
                    .remove(&leaf_node);

                // All parents that depend on this need to have their respective memory node be multiplied with the leaf
                // i.e., this is lifting the leaf into the parents
                for (parent, edge) in &parents_and_edges {
                    let parent_leaf = self.structure[*parent].ensure_leaf(index);
                    let memory = self.structure[*parent]
                        .get_memory(*edge)
                        .expect("Memory node was missing!");
                    self.structure[*parent].multiply_with_nodes(&vec![memory], &vec![parent_leaf]);
                }
            }
        }
    }

    /// Drop the leaf with `index` out of its current circuits into its ancestors.
    pub fn drop(&mut self, index: u32) {
        // Get all the circuits that depend on the leaf
        let dependencies = self.leafs[index as usize].get_dependencies();

        for dependency in dependencies {
            // If leaf is not a node within the circuit, go to next dependency
            // (i.e., this is just an ancestor dependency)
            let leaf_node;
            match self
                .structure
                .node_weight(dependency.into())
                .expect("Dependency was missing!")
                .get_leaf(index)
            {
                Some(node) => leaf_node = node,
                None => continue,
            }

            // Ensure that the dependant has at least one child, else we have no circuit to drop into
            let children_and_edges = self.ensure_child(dependency);

            // Drop leaf into child via multiplication and remove from this dependency
            for (child, _) in children_and_edges.iter() {
                self.structure
                    .node_weight_mut(*child)
                    .expect("Child AlgebraicCircuit was missin!")
                    .multiply(index);
                self.structure
                    .node_weight_mut(dependency.into())
                    .expect("Dependency was missing!")
                    .remove(&leaf_node);
            }
        }
    }

    /// Update the necessary values within the ReactiveCircuit and its output.
    pub fn update(&mut self) {
        for outdated_algebraic_circuit in self.queue.iter().rev() {
            let result = self
                .structure
                .node_weight((*outdated_algebraic_circuit).into())
                .expect("AlgebraicCircuit was missing!")
                .value(self);

            let edges: Vec<EdgeIndex> = self
                .structure
                .edges_directed((*outdated_algebraic_circuit).into(), Incoming)
                .map(|e| e.id())
                .collect();
            for edge in edges.iter() {
                self.structure
                    .edge_weight_mut(*edge)
                    .expect("ReactiveCircuit edge was missing!")
                    .assign(&result);
            }
        }
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
                    "ΣΠ\\n{}",
                    algebraic_circuit
                        .get_scope(&algebraic_circuit.root)
                        .iter()
                        .map(|leaf| {
                            if let NodeType::Leaf(index) = algebraic_circuit.structure[*leaf] {
                                format!("L{}", index)
                            } else {
                                "".to_string()
                            }
                        })
                        .collect::<Vec<String>>()
                        .join(" ")
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

    use crate::{channels::Vector, circuit::leaf::Leaf};

    use super::ReactiveCircuit;

    #[test]
    fn test_rc() -> std::io::Result<()> {
        let frequency = 1.0;
        let name = "";
        let value_size = 1;

        let leafs = vec![
            Leaf::new(Vector::ones(value_size), frequency, name),
            Leaf::new(Vector::ones(value_size), frequency, name),
            Leaf::new(Vector::ones(value_size), frequency, name),
        ];
        let mut reactive_circui =
            ReactiveCircuit::from_sum_product(value_size, &leafs, &vec![vec![0, 1], vec![0, 2]]);

        reactive_circui.to_svg("output/test/test_rc_original.svg", false)?;
        reactive_circui.structure.node_indices().for_each(|node| {
            let _ = reactive_circui.structure[node].to_svg(
                &format!("output/test/test_rc_ac_{}.svg", node.index()),
                false,
            );
        });
        reactive_circui.lift(0);
        reactive_circui.structure.node_indices().for_each(|node| {
            let _ = reactive_circui.structure[node].to_svg(
                &format!("output/test/test_rc_lift_l0_ac_{}.svg", node.index()),
                false,
            );
        });
        reactive_circui.to_svg("output/test/test_rc_lift_l1_rc.svg", false)?;

        Ok(())
    }
}
