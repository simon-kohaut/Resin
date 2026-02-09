use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::Write;
use std::mem::discriminant;
use std::process::Command;

use itertools::Itertools;
use petgraph::stable_graph::{EdgeIndex, Edges, NodeIndex, StableGraph};
use petgraph::visit::{EdgeRef, Walker};
use petgraph::visit::Bfs;
use petgraph::Direction::{Incoming, Outgoing};

use super::reactive::ReactiveCircuit;
use super::Vector;

#[derive(Clone, Debug, PartialEq)]
pub enum NodeType {
    Sum,
    Product,
    Leaf(u32),
    Memory(EdgeIndex),
}

#[derive(Clone, Debug)]
pub struct AlgebraicCircuit {
    pub(crate) structure: StableGraph<NodeType, ()>,
    pub(crate) root: NodeIndex,
    pub(crate) leafs: HashMap<u32, NodeIndex>,
    pub(crate) memories: HashMap<u32, NodeIndex>,
    value_size: usize,
}

impl AlgebraicCircuit {
    pub fn new(value_size: usize) -> Self {
        // Create a simple graph with a single sum node and nothing else
        let mut structure = StableGraph::new();
        let root = structure.add_node(NodeType::Sum);

        let algebraic_circuit = AlgebraicCircuit {
            structure,
            root,
            leafs: HashMap::new(),
            memories: HashMap::new(),
            value_size,
        };

        algebraic_circuit
    }

    /// Adds a set of leaf nodes node given their `indices` to the circuit's root.
    /// If some of the leafs are not yet part of the graph, new `NodeType::Leaf` nodes are added respectively.
    pub fn add(&mut self, indices: &[u32]) {
        let product: Vec<NodeIndex> = indices
            .iter()
            .map(|index| self.ensure_leaf(*index))
            .collect();
        self.add_to_node(&self.root.clone(), &product);
    }

    /// More efficient than calling `AlgebraicCircui::add` many times, attaching an entire `sum_product` to root. 
    pub fn add_sum_product(&mut self, sum_product: &[Vec<u32>]) {
        // Ensure that for each index a NodeType::Leaf(index) exists
        let indices = BTreeSet::from_iter(sum_product.iter().flatten());
        let mut leafs = HashMap::new();
        for index in indices {
            leafs.insert(index, self.ensure_leaf(*index));
        }

        // Add directly to root node with known leaf NodeIndex
        for product in sum_product {
            let leaf_product: Vec<NodeIndex> = product.iter().map(|index| leafs[index]).collect();
            self.add_to_node(&self.root.clone(), &leaf_product);
        }
    }

    /// Multiplies the circuit's root with a leaf node given its `index`.
    /// If no leaf with that `index` is within the circuit so far, a new `NodeType::Leaf` is added.
    pub fn multiply(&mut self, index: u32) {
        let factor = self.ensure_leaf(index);
        self.multiply_with_nodes(&vec![self.root], &vec![factor]);
    }

    /// Multiplies a `product` (slive of Leaf- or Memory-type nodes) with a set of `nodes` (Sum- or Product-type).
    ///
    /// All the `nodes` will be connected to the given `factor`, either directly in the case of
    /// `NodeType::Product` nodes or indirectly in the case of `NodeType::Sum` nodes.
    ///
    /// If `nodes` contains `NodeType::Leaf` or `NodeType::Memory` nodes, their parent
    /// products will be multiplied instead.
    ///
    /// This does not check if `product` is exlusively made up of `NodeType::Leaf` or `NodeType::Memory`,
    /// hence this might invalidate the circuit.
    /// It is also not ensured that all nodes in `product` are currently part of the circuit.
    /// If this is needed, use `multiply` instead.
    pub fn multiply_with_nodes(&mut self, nodes: &[NodeIndex], product: &[NodeIndex]) {
        for node in nodes {
            match self.structure[*node]
            {
                NodeType::Sum => {
                    let mut children = self.get_children(node);
                    if children.is_empty() {
                        let product_node = self.structure.add_node(NodeType::Product);
                        self.structure.add_edge(*node, product_node, ());
                        children.push(product_node);
                    }

                    self.multiply_with_nodes(&children, product);
                }
                NodeType::Product => {
                    for factor in product {
                        self.structure.add_edge(*node, *factor, ());
                    }
                }
                NodeType::Leaf(_) | NodeType::Memory(_) => {
                    self.multiply_with_nodes(&self.get_parents(node), product);
                }
            }
        }
    }

    /// Adds a `product` (slice of `NodeType::Leaf` or `NodeType::Memory` nodes) to the closest sum node.
    /// Returns the newly created product node.
    pub fn add_to_node(&mut self, node: &NodeIndex, product: &[NodeIndex]) -> NodeIndex {
        let product_node = match self
            .structure
            .node_weight(*node)
            .expect("Node was not found within Algebraic Circuit!")
        {
            NodeType::Sum => {
                let product_node = self.structure.add_node(NodeType::Product);
                self.structure.add_edge(*node, product_node, ());
                for factor in product {
                    self.structure.add_edge(product_node, *factor, ());
                }
                
                product_node
            }
            _ => self.add_to_node(&self.get_parents(node)[0], product)
        };

        product_node
    }

    /// Adds a `product` (slice of Leaf- or Memory-type nodes) to each node in `nodes`.
    /// For details, check the `AlgebraicCircuit::add_to_node` method.
    pub fn add_to_nodes(&mut self, nodes: &[NodeIndex], product: &[NodeIndex]) {
        for node in nodes {
            self.add_to_node(node, product);
        }
    }

    /// Create a new Algebraic Circuit from a `sum_product` expressed as a collection of collection of leaf indices.
    /// For each set of leaf indives, a product node is created.
    /// All product nodes are connected to a single sum node, which is the root of the new circuit.
    pub fn from_sum_product(value_size: usize, sum_product: &[Vec<u32>]) -> Self {
        // Initialize AlgebraicCircuit
        let mut algebraic_circuit = AlgebraicCircuit::new(value_size);
        
        // Add all the product nodes
        for product in sum_product {
            let leaf_product: Vec<NodeIndex> = product.iter().map(|index| algebraic_circuit.ensure_leaf(*index)).collect();
            algebraic_circuit.add_to_node(&algebraic_circuit.root.clone(), &leaf_product);
        }

        algebraic_circuit
    }

    /// Returns `Some(NodeIndex)` if a `NodeType::Leaf` with the given `index` was found in the circuit.
    /// Else, `None` is returned.
    pub fn get_leaf(&self, index: u32) -> Option<NodeIndex> {
        self.leafs.get(&index).copied()
    }

    /// Create and return a new memory node with the given `edge`.
    pub fn create_memory(&mut self, edge: EdgeIndex) -> NodeIndex {
        let memory_node = self.structure.add_node(NodeType::Memory(edge));
        self.memories.insert(edge.index() as u32, memory_node);
        memory_node
    }

    /// Returns `Some(NodeIndex)` if a `NodeType::Memory` with the given `index` was found in the circuit.
    /// Else, `None` is returned.
    pub fn get_memory(&self, index: EdgeIndex) -> Option<NodeIndex> {
        self.memories.get(&(index.index() as u32)).copied()
    }

    /// Checks if a `NodeType::Leaf` with `index` is part of this circuit.
    pub fn is_in_circuit(&self, index: u32) -> bool {
        match self.leafs.get(&index) {
            Some(_) => true,
            None => false
        }
    }

    /// Get the scope, i.e., the set of leafs and memory nodes that are part of the computation of the given `node`.
    /// The scope is reported as `Vec<NodeIndex>`.
    pub fn get_scope(&self, node: &NodeIndex) -> BTreeSet<NodeIndex> {
        let mut scope = BTreeSet::new();

        let mut bfs = Bfs::new(&self.structure, *node);
        while let Some(descendant) = bfs.next(&self.structure) {
            match self.structure.node_weight(descendant).unwrap() { 
                NodeType::Leaf(_) | NodeType::Memory(_) => {
                    scope.insert(descendant);
                },
                _ => {}
            }
        }

        scope
    }

    /// Iterate over the parent nodes of the given `node` within this circuit.
    pub fn iter_parents(&self, node: &NodeIndex) -> impl Iterator<Item = NodeIndex> + '_ {
        self.structure.neighbors_directed(*node, Incoming)
    }

    /// Iterate over the child nodes of the given `node` within this circuit.
    pub fn iter_children(&self, node: &NodeIndex) -> impl Iterator<Item = NodeIndex> + '_ {
        self.structure.neighbors_directed(*node, Outgoing)
    }

    /// Iterate over the granparents of the given `node` within this circuit.
    pub fn iter_grandparent(&self, node: &NodeIndex) -> impl Iterator<Item = NodeIndex> + '_ {
        self.iter_parents(node).flat_map(|parent| self.iter_parents(&parent))
    }

    /// Iterate over the siblings of the given `node` within this circuit.
    pub fn iter_siblings(&self, node: &NodeIndex) -> impl Iterator<Item = NodeIndex> + '_ {
        let node_to_exclude = *node;
        self.iter_parents(node)
            .flat_map(|parent| self.iter_children(&parent))
            .filter(move |&sibling| sibling != node_to_exclude)
    }

    /// Get all the parent nodes of the given `node` within this circuit.
    pub fn get_parents(&self, node: &NodeIndex) -> Vec<NodeIndex> {
        self.iter_parents(node).unique().collect()
    }

    /// Get all the child nodes of the given `node` within this circuit.
    pub fn get_children(&self, node: &NodeIndex) -> Vec<NodeIndex> {
        self.iter_children(node).unique().collect()
    }

    /// Get all the grandparent nodes of the given `node` within this circuit.
    pub fn get_grandparents(&self, node: &NodeIndex) -> Vec<NodeIndex> {
        self.iter_grandparent(node).unique().collect()
    }

    /// Get all the sibling nodes of the given `node`, i.e., those with a shared parent, within this circuit.
    pub fn get_siblings(&self, node: &NodeIndex) -> Vec<NodeIndex> {
        self.iter_siblings(node).unique().collect()
    }

    /// Remove all edges that may connect nodes `a` and `b`.
    pub fn disconnect_nodes(&mut self, a: &NodeIndex, b: &NodeIndex) {
        while let Some(edge_id) = self.structure.find_edge(*a, *b) {
            self.structure.remove_edge(edge_id);
        }
        while let Some(edge_id) = self.structure.find_edge(*b, *a) {
            self.structure.remove_edge(edge_id);
        }
    }

    pub fn disconnect_from_parents(&mut self, node: &NodeIndex) {
        let parents = self.get_parents(node);
        for parent in parents.iter() {
            self.structure.remove_edge(
                self.structure
                    .edges_connecting(*parent, *node)
                    .next()
                    .unwrap()
                    .id(),
            );
        }
    }

    /// Check if `node` is of type `node_tupe`.
    pub fn check_node_type(&self, node: &NodeIndex, node_type: &NodeType) -> bool {
        discriminant(self.structure.node_weight(*node).unwrap()) == discriminant(node_type)
    }

    /// Filters the list of all `nodes` for the ones that have the given `node_type`.
    pub fn filter_nodes_by_type(
        &self,
        nodes: &[NodeIndex],
        node_type: &NodeType,
    ) -> Vec<NodeIndex> {
        nodes
            .iter()
            .filter(|node| self.check_node_type(node, node_type))
            .cloned()
            .collect()
    }

    /// Returns a list of those nodes in `nodes` that have `leaf` in scope.
    pub fn filter_nodes_by_scope(
        &self,
        nodes: &[NodeIndex],
        leaf: &NodeIndex,
    ) -> (Vec<NodeIndex>, Vec<NodeIndex>) {
        let mut in_scope_nodes = Vec::new();
        let mut out_of_scope_nodes = Vec::new();

        for node in nodes.iter() {
            match self.is_in_scope(node, leaf) {
                true => in_scope_nodes.push(*node),
                false => out_of_scope_nodes.push(*node),
            }
        }

        (in_scope_nodes, out_of_scope_nodes)
    }

    /// Finds the next ancestor of `node` within the circuit that has the given `node_type`.
    fn find_next_ancestors_by_type(
        &self,
        node: &NodeIndex,
        node_type: &NodeType,
    ) -> Vec<NodeIndex> {
        // Get all the parents of given node
        let parents: Vec<NodeIndex> = self
            .structure
            .edges_directed(*node, Incoming)
            .map(|edge| edge.source())
            .collect();

        // Either there are no ancestores at all ...
        if parents.is_empty() {
            Vec::new()
        }
        // ... the parents do not match the desired node_type, then we go further up ...
        // (This assumes all parents will have the same type)
        else if !self.check_node_type(&parents[0], node_type) {
            let mut combined = Vec::new();
            for parent in &parents {
                combined.extend(self.find_next_ancestors_by_type(parent, &node_type));
            }

            combined
        }
        // ... or we found the right ancestors
        else {
            parents
        }
    }

    // Remove the `NodeType::Leaf` node with `index` from this circuit.
    pub fn remove(&mut self, node: &NodeIndex) {
        self.leafs.retain(|_, leaf| leaf != node);
        self.memories.retain(|_, memory| memory != node);
        self.structure.remove_node(*node);
    }

    pub fn remove_disconnected(&mut self) {
        let mut to_be_removed = Vec::new();
        for node in self.structure.node_indices() {
            let number_of_incoming_edges =  self
                .structure
                .edges_directed(node, Incoming)
                .count();
            let number_of_outgoing_edges =  self
                .structure
                .edges_directed(node, Outgoing)
                .count();

            if number_of_incoming_edges == 0 && number_of_outgoing_edges == 0 {
                to_be_removed.push(node);
            }
        }

        while let Some(node) = to_be_removed.pop() {
            self.remove(&node);
        }
    }

    /// Removes a `node` and all of its descendants.
    /// `NodeType::Leaf` and `NodeType::Memory` are only deleted if no other branch of the graph depends on them
    fn remove_with_descendants(&mut self, node: &NodeIndex) {
        let descendants: Vec<NodeIndex> = Bfs::new(&self.structure, *node).iter(&self.structure).collect();

        for descendant in descendants {
            match self.structure.node_weight(descendant).unwrap() {
                NodeType::Leaf(_) | NodeType::Memory(_) => {},
                _ => {
                    self.remove(&descendant);
                }
            }
        }
        self.remove(node);
    }

    /// Removes all edges that point towards the given `node`.
    fn remove_incoming_edges(&mut self, node: &NodeIndex) {
        // Collect all incoming edges to the node
        let incoming_edges: Vec<_> = self
            .structure
            .edges_directed(*node, Incoming)
            .map(|edge| edge.id())
            .collect();

        // Remove each incoming edge using its ID
        for edge_id in incoming_edges.iter() {
            self.structure.remove_edge(*edge_id);
        }
    }

    /// Removes all edges that originate from the given `node`.
    fn remove_outgoing_edges(&mut self, node: &NodeIndex) {
        // Collect all outgoind edges from the node
        let outgoing_edges: Vec<_> = self
            .structure
            .edges_directed(*node, Outgoing)
            .map(|edge| edge.id())
            .collect();

        // Remove each outgoing edge using its ID
        for edge_id in outgoing_edges.iter() {
            self.structure.remove_edge(*edge_id);
        }
    }

    /// Checks if the node `b` is within the scope of the node `a`.
    pub fn is_in_scope(&self, a: &NodeIndex, b: &NodeIndex) -> bool {
        self.get_scope(a).contains(b)
    }

    /// Checks if the set of parents of the `node` is empty.
    fn is_orphan(&self, node: &NodeIndex) -> bool {
        self.get_parents(node).is_empty()
    }

    /// Checks if the set of children of the `node` is empty.
    fn is_childless(&self, node: &NodeIndex) -> bool {
        self.get_children(node).is_empty()
    }

    /// Looks up the `NodeIndex` of a `NodeType::Leaf` with the given `index`.
    /// If it is not found within the circuit, a new node is created and the new `NodeIndex` returned.
    pub fn ensure_leaf(&mut self, index: u32) -> NodeIndex {
        let leaf = self.get_leaf(index);
        match leaf {
            Some(leaf) => leaf,
            None => {
                let new_node = self.structure.add_node(NodeType::Leaf(index));
                self.leafs.insert(index, new_node);
                new_node
            }
        }
    }

    /// Splits the Algebraic Circuit into one that contains the `node` and one that does not.
    pub fn split(
        &mut self,
        index: u32,
    ) -> (Option<AlgebraicCircuit>, Option<AlgebraicCircuit>) {
        // New structure is a sum over two products, each with just one sum of which
        // one contains the node and one doesnt
        let leaf = self.get_leaf(index).unwrap();
        let (in_scope_root, out_of_scope_root) = self.split_sum(&self.root.clone(), &leaf);

        // We create a clone of the graph for each new circuit
        // If the other variant exists we delete the respective sub-graph
        // e.g., we delete the out-of-scope part from the in-scope circuit
        let mut in_scope_circuit = None;
        if in_scope_root.is_some() {
            let mut new_circuit = self.clone();
            new_circuit.root = in_scope_root.unwrap();

            if out_of_scope_root.is_some() {
                new_circuit.remove_with_descendants(&out_of_scope_root.unwrap());
            }
            new_circuit.remove_disconnected();

            in_scope_circuit = Some(new_circuit);
            assert!(in_scope_circuit.as_ref().unwrap().structure.node_indices().contains(&in_scope_root.unwrap()));
        }

        let mut out_of_scope_circuit = None;
        if out_of_scope_root.is_some() {
            let mut new_circuit = self.clone();
            new_circuit.root = out_of_scope_root.unwrap();
            
            if in_scope_root.is_some() {
                new_circuit.remove_with_descendants(&in_scope_root.unwrap());
            }
            new_circuit.remove_disconnected();

            out_of_scope_circuit = Some(new_circuit);
            assert!(out_of_scope_circuit.as_ref().unwrap().structure.node_indices().contains(&out_of_scope_root.unwrap()));
        }

        (in_scope_circuit, out_of_scope_circuit)
    }

    /// Splits a `sum_node` into two, one with `node` in scope and one without.
    /// Returns an `Option<NodeIndex>` for both new sum-nodes, the first having `node` in scope.
    fn split_sum(
        &mut self,
        sum_node: &NodeIndex,
        node: &NodeIndex,
    ) -> (Option<NodeIndex>, Option<NodeIndex>) {
        // Get children and parent of sum node and remove it from circuit
        let products = self.get_children(sum_node);
        let parents = self.get_parents(sum_node);

        // Separate products by their scope, either containing the node or not
        let (in_scope_products, out_of_scope_products) =
            self.filter_nodes_by_scope(&products, node);

        // Create and connect the new separate sums
        let mut in_scope_sum = None;
        if !in_scope_products.is_empty() {
            in_scope_sum = Some(self.structure.add_node(NodeType::Sum));

            for product in in_scope_products.iter() {
                self.structure.add_edge(in_scope_sum.unwrap(), *product, ());
            }

            for parent in parents.iter() {
                self.structure.add_edge(*parent, in_scope_sum.unwrap(), ());
            }
        }

        let mut out_of_scope_sum = None;
        if !out_of_scope_products.is_empty() {
            out_of_scope_sum = Some(self.structure.add_node(NodeType::Sum));

            for product in out_of_scope_products.iter() {
                self.structure
                    .add_edge(out_of_scope_sum.unwrap(), *product, ());
            }

            for parent in parents.iter() {
                self.structure
                    .add_edge(*parent, out_of_scope_sum.unwrap(), ());
            }
        }

        self.remove(sum_node);
        (in_scope_sum, out_of_scope_sum)
    }

    /// Applies the distributive law on a `node`.
    /// 
    /// For example, if the circuit represents the formula `a * (b + c)`, the circuit will be `(a * b) + (a * c)`.
    /// This function does not check if `node` has `NodeType::Leaf` or `NodeType::Memory`.
    /// If this is not the case, the resulting circuit will be invalid.
    fn factor_in(&mut self, node: &NodeIndex) {
        let products = self.get_parents(node);

        for product in &products {
            self.disconnect_nodes(product, node);

            let sum_children =
                self.filter_nodes_by_type(&self.get_children(product), &NodeType::Sum);

            if sum_children.is_empty() {
                let sum = self.structure.add_node(NodeType::Sum);
                let inner_product = self.structure.add_node(NodeType::Product);
                self.structure.add_edge(*product, sum, ());
                self.structure.add_edge(sum, inner_product, ());
                self.structure.add_edge(inner_product, *node, ());
            } else {
                self.multiply_with_nodes(&sum_children, &vec![*node]);
            }
        }
    }

    /// Applies the distributive law on a `node`.
    ///
    /// For example, if the circuit represents the formula `(a * b) + (a * c)`, the circuit will be `a * (b + c)`.
    /// Or, if the circuit represents the formula `(a * b) + (d * c)`, it will be `a * (b) + (d * c)`.
    ///
    /// This function does not check if `node` has `NodeType::Leaf` or `NodeType::Memory`.
    /// If this is not the case, the resulting circuit will be invalid.
    fn factor_out(&mut self, node: &NodeIndex) {
        for sum_node in self.get_grandparents(node).iter() {
            // Remove this nodes incoming edges
            self.remove_incoming_edges(node);

            // Ensure all grandparents have parents to factor into and add the node to the new product
            if self.is_orphan(sum_node) {
                let new_sum = self.structure.add_node(NodeType::Sum);
                let new_product = self.structure.add_node(NodeType::Product);
                self.structure.add_edge(new_sum, new_product, ());
                self.structure.add_edge(new_product, *sum_node, ());
                self.structure.add_edge(new_product, *node, ());
            }
            // Else we can multiply with the sums existing parent products
            else {
                self.multiply_with_nodes(&self.get_parents(sum_node), &vec![*node]);
            }
        }
    }

    /// Get the value of this Algebraic Circuit, i.e., calling `node_value` on the `root` node.
    /// The `reactive_circuit` is used to read memorized results from other Algebraic Circuits and leaf values.
    pub fn value(&self, reactive_circuit: &ReactiveCircuit) -> Vector {
        self.node_value(&self.root, reactive_circuit)
    }

    /// Computes the value of a `node` given its `NodeType` and a `reactive_circuit` containing leaf and memorized values.
    pub fn node_value(&self, node: &NodeIndex, reactive_circuit: &ReactiveCircuit) -> Vector {
        match self.structure[*node]
        {
            NodeType::Leaf(index) => return reactive_circuit.leafs[index as usize].get_value(),
            NodeType::Product => {
                let mut result = Vector::ones(self.value_size);
                self.iter_children(node)
                    .for_each(|child| result *= &self.node_value(&child, reactive_circuit));

                result
            }
            NodeType::Sum => {
                let mut result = Vector::zeros(self.value_size);
                self.iter_children(node)
                    .for_each(|child| result += &self.node_value(&child, reactive_circuit));

                result                
            }
            NodeType::Memory(edge) => {
                return reactive_circuit.structure[edge].clone()
            }
        }
    }

    /// Merge all the `NodeType::Sum` children of a `NodeType::Product` into one
    pub fn merge_sums(&mut self, node: &NodeIndex) {
        let sums = self.filter_nodes_by_type(&self.get_children(node), &NodeType::Sum);

        let replacement;
        if sums.is_empty() {
            return;
        } else {
            replacement = self.structure.add_node(NodeType::Sum);
        }

        for sum in &sums {
            self.remove(sum);
            for product in &self.get_children(sum) {
                self.structure.add_edge(replacement, *product, ());

                let grandchildren_sums =
                    self.filter_nodes_by_type(&self.get_children(product), &NodeType::Sum);
                for grandchild_sum in &grandchildren_sums {
                    self.merge_sums(grandchild_sum);
                }
            }
        }
    }

    /// Remove all nodes with no incoming or outgoing edges.
    /// If a product has multiple sum nodes as children, they are merged into one.
    pub fn prune(&mut self) {
        loop {
            // Collect nodes without use
            let mut nodes_to_remove = Vec::new();
            for node in self.structure.node_indices() {
                match self.structure[node]
                {
                    NodeType::Leaf(_) | NodeType::Memory(_) => {
                        if self.structure.edges_directed(node, Incoming).count() == 0 {
                            nodes_to_remove.push(node.to_owned());
                        }
                    },
                    NodeType::Product | NodeType::Sum => {
                        if self.structure.edges_directed(node, Outgoing).count() == 0 {
                            nodes_to_remove.push(node.to_owned());
                        }
                    }
                }
            }

            // Check if we are done
            if nodes_to_remove.is_empty() {
                break;
            }

            // Remove the nodes if necessary and repeat
            for node in nodes_to_remove {
                self.remove(&node);
            }
        }
    }

    /// Compile AlgebraicCircuit into dot format text and return as `String`.
    pub fn to_dot_text(&self) -> String {
        let mut dot = String::new();

        // Start the DOT graph
        dot.push_str("digraph AlgebraicCircuit {\n");
        dot.push_str("    node [margin=0 penwidth=2];\n");
        dot.push_str("    edge [color=\"gray20\" penwidth=2];\n");

        // Iterate over the nodes
        let mut bfs = Bfs::new(&self.structure, self.root);
        while let Some(node) = bfs.next(&self.structure) {
            let node_type = &self.structure[node];
            let node_label = match node_type {
                NodeType::Sum => "Σ".to_string(),
                NodeType::Product => "Π".to_string(),
                NodeType::Leaf(index) => format!("L{}", index),
                NodeType::Memory(edge) => format!("M{}", edge.index()),
            };
            let node_shape = match node_type {
                NodeType::Memory(_) => "square",
                _ => "circle",
            };
            let node_color = match node_type {
                NodeType::Sum => "crimson",
                NodeType::Product => "dodgerblue",
                NodeType::Leaf(_) | NodeType::Memory(_) => "darkorchid",
            };
            dot.push_str(&format!(
                "    {} [shape=\"{}\" color=\"{}\" label=\"{}\"];\n",
                node.index(),
                node_shape,
                node_color,
                node_label
            ));
        }

        // Iterate over the edges
        for edge in self.structure.edge_indices() {
            let (source, target) = self.structure.edge_endpoints(edge).unwrap();
            dot.push_str(&format!("    {} -> {};\n", source.index(), target.index()));
        }

        // End the DOT graph
        dot.push_str("}\n");
        dot
    }

    /// Write out the AlgebraicCircuit as dot file at the given `path`.
    pub fn to_dot(&self, path: &str) -> std::io::Result<()> {
        // Translate graph into DOT text
        let dot = self.to_dot_text();

        // Write to disk
        let mut file = File::create(path)?;
        file.write_all(dot.as_bytes())?;
        Ok(())
    }

    /// Write out the AlgebraicCircuit as svg file at the given `path`.
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
}

#[cfg(test)]
mod tests {

    use std::collections::BTreeSet;
    use ndarray::array;

    use crate::circuit::leaf::Leaf;
    use crate::circuit::reactive::ReactiveCircuit;

    use super::{AlgebraicCircuit, NodeType};

    #[test]
    fn test_ac() -> std::io::Result<()> {
        // Create a simple formula a * b + a * c
        let mut ac = AlgebraicCircuit::new(1);
        ac.add(&vec![0, 1]);
        ac.add(&vec![0, 2]);

        let a = ac.get_leaf(0).unwrap();
        let b = ac.get_leaf(1).unwrap();
        let c = ac.get_leaf(2).unwrap();

        // 3 Leafs + 2 Products + 1 Sum = 6 Nodes total with 6 edges
        assert_eq!(ac.structure.node_indices().count(), 6);
        assert_eq!(ac.structure.edge_indices().count(), 6);

        // The scope should consist of the Leaf nodes 0, 1 and 2
        assert_eq!(
            ac.get_scope(&ac.root),
            BTreeSet::from_iter(vec![a, b, c])
        );
        assert_eq!(
            ac.filter_nodes_by_type(&Vec::from_iter(ac.get_scope(&ac.root)), &NodeType::Leaf(0))
                .len(),
            3
        );

        // Leaf 0 is part of 2 products, leafs 1 and 2 each have only 1 product parent
        assert_eq!(ac.get_parents(&a).len(), 2);
        assert_eq!(ac.get_parents(&b).len(), 1);
        assert_eq!(ac.get_parents(&c).len(), 1);

        // There is only 1 grandparent, i.e., the sum as root node
        assert_eq!(ac.get_grandparents(&a).len(), 1);
        assert_eq!(ac.get_grandparents(&b).len(), 1);
        assert_eq!(ac.get_grandparents(&c).len(), 1);
        assert_eq!(ac.get_grandparents(&c)[0], ac.root);

        // The children of parents are the input nodes
        for parent in ac.get_parents(&b).iter() {
            assert_eq!(ac.get_children(parent), vec![b, a]);
        }
        for parent in ac.get_parents(&c).iter() {
            assert_eq!(ac.get_children(parent), vec![c, a]);
        }

        // Leaf 0 is the sibling of the other 2
        assert_eq!(BTreeSet::from_iter(ac.get_siblings(&a)), BTreeSet::from_iter(vec![b, c]));
        assert_eq!(ac.get_siblings(&b), vec![a]);
        assert_eq!(ac.get_siblings(&c), vec![a]);

        // Write original circuit as SVG
        ac.to_svg("output/test/test_ac_original.svg", false)?;

        // Factor out leaf 0, i.e., the other leafs should be deeper than leaf 0 afterwards
        ac.factor_out(&ac.get_leaf(0).unwrap());

        // Write new circuit as SVG
        ac.to_svg("output/test/test_ac_factored_out_l0.svg", false)?;

        Ok(())
    }

    #[test]
    fn test_value_computation() {
        // Formula: 0*1 + 0*2
        let sum_product = vec![vec![0, 1], vec![0, 2]];

        // 1. Setup Reactive Circuit with leaf values
        let mut reactive_circuit = ReactiveCircuit::new(1);
        reactive_circuit.leafs.push(Leaf::new(array![0.5].into(), 0.0, "l0"));
        reactive_circuit.leafs.push(Leaf::new(array![0.2].into(), 0.0, "l1"));
        reactive_circuit.leafs.push(Leaf::new(array![0.8].into(), 0.0, "l2"));

        // 2. Create Algebraic Circuit from formula
        let ac = AlgebraicCircuit::from_sum_product(1, &sum_product);

        // 3. Calculate expected value and compare
        let expected_value = 0.5 * 0.2 + 0.5 * 0.8; // 0.1 + 0.4 = 0.5
        let computed_value = ac.value(&reactive_circuit);

        assert!((computed_value[0] - expected_value).abs() < 1e-9);
    }

    #[test]
    fn test_factor_in() -> std::io::Result<()> {
        // Create a circuit for a * (b + c)
        let mut ac = AlgebraicCircuit::new(1);
        let product_node = ac.structure.add_node(NodeType::Product);
        ac.structure.add_edge(ac.root, product_node, ());

        let leaf_a = ac.ensure_leaf(0);
        ac.structure.add_edge(product_node, leaf_a, ());

        let sum_node = ac.structure.add_node(NodeType::Sum);
        ac.structure.add_edge(product_node, sum_node, ());

        let product_b = ac.structure.add_node(NodeType::Product);
        let leaf_b = ac.ensure_leaf(1);
        ac.structure.add_edge(sum_node, product_b, ());
        ac.structure.add_edge(product_b, leaf_b, ());

        let product_c = ac.structure.add_node(NodeType::Product);
        let leaf_c = ac.ensure_leaf(2);
        ac.structure.add_edge(sum_node, product_c, ());
        ac.structure.add_edge(product_c, leaf_c, ());

        ac.to_svg("output/test/test_ac_factor_in_before.svg", false)?;

        // Factor in 'a' to get (a*b) + (a*c)
        ac.factor_in(&leaf_a);
        ac.prune();

        ac.to_svg("output/test/test_ac_factor_in_after.svg", false)?;

        // Check products
        assert_eq!(ac.get_siblings(&leaf_a), vec![leaf_b, leaf_c]);

        Ok(())
    }

    #[test]
    fn test_prune_and_remove() {
        let mut ac = AlgebraicCircuit::new(1);
        ac.add(&vec![0, 1]);
        ac.add(&vec![0, 2]);
        ac.add(&vec![3, 4]); // An extra product

        assert_eq!(ac.structure.node_count(), 9); // 1 Sum, 3 Products, 5 Leafs

        // Remove leaf 1. The product (0,1) should become just (0)
        let leaf_1 = ac.get_leaf(1).unwrap();
        ac.remove(&leaf_1);
        ac.prune();

        assert!(!ac.leafs.contains_key(&1));
        assert_eq!(ac.structure.node_count(), 8); // Leaf 1 is gone

        // Remove product (3,4) by removing its leafs
        let leaf_3 = ac.get_leaf(3).unwrap();
        let leaf_4 = ac.get_leaf(4).unwrap();
        ac.remove(&leaf_3);
        ac.remove(&leaf_4);
        ac.prune();

        assert_eq!(ac.structure.node_count(), 5); // 1 Sum, 2 Products, 2 Leafs (0, 2)
    }

    #[test]
    fn test_split() -> std::io::Result<()> {
        // Create a simple formula a * b + a * c
        let mut original = AlgebraicCircuit::new(1);
        original.add(&vec![0, 1]);
        original.add(&vec![0, 2]);

        let leaf_0 = original.get_leaf(0).unwrap();
        let leaf_1 = original.get_leaf(1).unwrap();
        let leaf_2 = original.get_leaf(2).unwrap();

        original.to_svg("output/test/test_split_original.svg", false)?;

        // Test splitting
        // First case: This should do nothing, as all products contain leaf 0
        let mut ac = original.clone();
        let (in_scope_ac, out_of_scope_ac) = ac.split_sum(&ac.root.clone(), &leaf_0);
        assert_eq!(
            ac.get_scope(in_scope_ac.as_ref().unwrap()),
            BTreeSet::from_iter(vec![leaf_0, leaf_1, leaf_2])
        );
        assert!(out_of_scope_ac.is_none());

        // Second case: This should create a new root
        let mut ac = original.clone();
        let (in_scope_ac, out_of_scope_ac) = ac.split_sum(&ac.root.clone(), &leaf_1);
        assert_eq!(
            ac.get_scope(in_scope_ac.as_ref().unwrap()),
            BTreeSet::from_iter(vec![leaf_0, leaf_1])
        );
        assert_eq!(
            ac.get_scope(out_of_scope_ac.as_ref().unwrap()),
            BTreeSet::from_iter(vec![leaf_0, leaf_2])
        );

        // Third case: We apply split to the entire circuit
        let mut ac = original.clone();
        let (in_scope_ac, out_of_scope_ac) = ac.split(1);
        let mut in_scope_ac = in_scope_ac.unwrap();
        in_scope_ac.to_svg("output/test/test_split_in_scope_ac_leaf_1.svg", false)?;
        let mut out_of_scope_ac = out_of_scope_ac.unwrap();
        out_of_scope_ac.to_svg("output/test/test_split_out_of_scope_ac_leaf_1.svg", false)?;
        
        let new_leaf_0 = in_scope_ac.ensure_leaf(0);
        let new_leaf_1 = in_scope_ac.ensure_leaf(1);
        let new_leaf_2 = in_scope_ac.ensure_leaf(2);
        assert!(in_scope_ac.is_in_scope(&in_scope_ac.root, &new_leaf_0));
        assert!(in_scope_ac.is_in_scope(&in_scope_ac.root, &new_leaf_1));
        assert!(!in_scope_ac.is_in_scope(&in_scope_ac.root, &new_leaf_2));

        let new_leaf_0 = out_of_scope_ac.ensure_leaf(0);
        let new_leaf_1 = out_of_scope_ac.ensure_leaf(1);
        let new_leaf_2 = out_of_scope_ac.ensure_leaf(2);
        assert!(out_of_scope_ac.is_in_scope(&out_of_scope_ac.root, &new_leaf_0));
        assert!(!out_of_scope_ac.is_in_scope(&out_of_scope_ac.root, &new_leaf_1));
        assert!(out_of_scope_ac.is_in_scope(&out_of_scope_ac.root, &new_leaf_2));

        Ok(())
    }
}
