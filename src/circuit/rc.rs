use std::fs::File;
use std::io::Write;
use std::mem::discriminant;
use std::process::Command;

use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::EdgeRef;
use petgraph::Direction::{Incoming, Outgoing};

use rayon::iter::IntoParallelRefIterator;
use rayon::iter::ParallelIterator;

use super::Vector;

#[derive(Debug, PartialEq)]
enum NodeType {
    Memory(Vector, bool),
    Sum,
    Product,
    Leaf(usize),
}

#[derive(Debug)]
struct ReactiveCircuit {
    structure: StableGraph<NodeType, ()>,
    leafs: Vec<NodeIndex>,
    products: Vec<NodeIndex>,
}

impl ReactiveCircuit {

    pub fn new() -> Self {
        ReactiveCircuit {
            structure: StableGraph::new(),
            leafs: Vec::new(),
            products: Vec::new(),
        }
    }

    pub fn from_sum_product(sum_product: &[Vec<usize>]) -> Self {
        // Initialize ReactiveCircuit
        let mut rc = ReactiveCircuit::new();

        // Add single memorized sum node
        let memory_index = rc
            .structure
            .add_node(NodeType::Memory(Vector::from(vec![1.0]), false));
        let sum_index = rc.structure.add_node(NodeType::Sum);
        rc.structure.add_edge(memory_index, sum_index, ());

        // Add the product nodes
        for product in sum_product {
            let product_index = rc.structure.add_node(NodeType::Product);
            rc.structure.add_edge(sum_index, product_index, ());
            rc.products.push(product_index);

            for leaf in product {
                match rc.leafs.iter().find(|node| {
                    *rc.structure.node_weight(**node).unwrap() == NodeType::Leaf(*leaf)
                }) {
                    Some(leaf_index) => {
                        rc.structure.add_edge(product_index, *leaf_index, ());
                    }
                    None => {
                        let leaf_index = rc.structure.add_node(NodeType::Leaf(*leaf));
                        rc.structure.add_edge(product_index, leaf_index, ());
                        rc.leafs.push(leaf_index);
                    }
                }
            }
        }

        rc
    }

    fn find_leaf(&self, index: usize) -> Option<NodeIndex> {
        // Check which NodeIndex belongs to this leaf
        let mut leaf_index = None;
        for leaf in &self.leafs {
            if NodeType::Leaf(index) == self.structure[*leaf] {
                leaf_index = Some(*leaf);
                break;
            }
        }

        leaf_index
    }

    fn find_products_containing_leaf(&self, index: usize) -> Option<Vec<NodeIndex>> {
        let node = self.find_leaf(index);
        match node {
            Some(node) => Some(self.get_parents(&node)),
            None => None,
        }
    }

    fn create_empty_sub_graph(&mut self) -> (NodeIndex, NodeIndex, NodeIndex) {
        // Add single memorized sum and product nodes
        let new_memory = self
            .structure
            .add_node(NodeType::Memory(Vector::from(vec![1.0]), false));
        let new_sum = self.structure.add_node(NodeType::Sum);
        let new_product = self.structure.add_node(NodeType::Product);

        // Memorize product
        self.products.push(new_product);

        // Add edges
        self.structure.add_edge(new_memory, new_sum, ());
        self.structure.add_edge(new_sum, new_product, ());

        (new_memory, new_sum, new_product)
    }

    fn get_parents(&self, node: &NodeIndex) -> Vec<NodeIndex> {
        let parents: Vec<NodeIndex> = self
            .structure
            .edges_directed(*node, Incoming)
            .map(|edge| edge.source())
            .collect();

        // All parents need to have same type within RC
        debug_assert!(
            parents.is_empty()
                || parents.len()
                    == self
                        .filter_nodes_by_type(
                            &parents,
                            &self.structure.node_weight(parents[0]).unwrap()
                        )
                        .len(),
            "Found mix of node types among a set of parents in RC!"
        );

        parents
    }

    fn get_children(&self, node: &NodeIndex) -> Vec<NodeIndex> {
        self.structure
            .edges_directed(*node, Outgoing)
            .map(|edge| edge.source())
            .collect()
    }

    fn get_siblings(&self, node: &NodeIndex) -> Vec<NodeIndex> {
        let mut siblings = Vec::new();
        for parent in &self.get_parents(node) {
            siblings.extend(self.get_children(parent));
        }

        siblings
    }

    fn check_node_type(&self, node: &NodeIndex, node_type: &NodeType) -> bool {
        discriminant(self.structure.node_weight(*node).unwrap()) == discriminant(node_type)
    }

    fn filter_nodes_by_type(&self, nodes: &[NodeIndex], node_type: &NodeType) -> Vec<NodeIndex> {
        nodes
            .iter()
            .filter(|node| self.check_node_type(node, node_type))
            .cloned()
            .collect()
    }

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

    fn remove_incoming_edges(&mut self, node: &NodeIndex) {
        // Collect all incoming edges to the node
        let incoming_edges: Vec<_> = self
            .structure
            .edges_directed(*node, Incoming)
            .map(|edge| edge.id())
            .collect();

        // Remove each incoming edge using its ID
        for edge_id in incoming_edges {
            self.structure.remove_edge(edge_id);
        }
    }

    fn is_orphan(&self, node: &NodeIndex) -> bool {
        self.get_parents(node).is_empty()
    }

    fn is_childless(&self, node: &NodeIndex) -> bool {
        self.get_children(node).is_empty()
    }

    fn is_at_top(&self, node: &NodeIndex) -> bool {
        match self.structure.node_weight(*node) {
            // If memory node, its at the top if it has no parents
            Some(NodeType::Memory(..)) => self.is_orphan(node),
            // If not in graph, its also not at the top
            None => false,
            // Otherwise, it is at the top if we cannot find a direct Memory ancestor that is not an orphan
            _ => self
                .find_next_ancestors_by_type(
                    node,
                    &NodeType::Memory(Vector::from(vec![1.0]), false),
                )
                .iter()
                .find(|ancestor| !self.is_orphan(&ancestor))
                .is_none(),
        }
    }

    fn ensure_sub_graph_above(&mut self, node: &NodeIndex) {
        // Collect next memory nodes above
        let memory_node_ancestors: Vec<NodeIndex> = self
            .find_next_ancestors_by_type(node, &NodeType::Memory(Vector::from(vec![1.0]), false));

        // If they are orphans, create a new sub-graph above
        for memory_node in &memory_node_ancestors {
            if self.is_orphan(memory_node) {
                let (_, _, new_product) = self.create_empty_sub_graph();
                self.structure.add_edge(new_product, *memory_node, ());
            }
        }
    }

    fn ensure_sub_graph_below(&mut self, node: &NodeIndex) {
        // For each type of node we get the next related product node(s)
        match self.structure.node_weight(*node) {
            // For a leaf, its the parents
            Some(NodeType::Leaf(_)) => self
                .get_parents(node)
                .iter()
                .for_each(|product| self.ensure_sub_graph_below(product)),
            // For a sum, its the children
            Some(NodeType::Sum) => self
                .get_children(node)
                .iter()
                .for_each(|product| self.ensure_sub_graph_below(product)),
            // For a memory cell, its the grandchildren
            Some(NodeType::Memory(..)) => {
                let sum_children = self.get_children(node);
                for sum_child in &sum_children {
                    self.get_children(sum_child)
                        .iter()
                        .for_each(|product| self.ensure_sub_graph_below(product));
                }
            }
            Some(NodeType::Product) => {
                let children = self.get_children(node);

                // If the product is within a larger sub-graph (pointing at a sum instead of memory), we delegate further down
                if !self
                    .filter_nodes_by_type(&children, &NodeType::Sum)
                    .is_empty()
                {
                    self.ensure_sub_graph_below(node);
                }

                // Else, we check if it points at memory and add the missing sub-graph if not
                if self
                    .filter_nodes_by_type(
                        &children,
                        &NodeType::Memory(Vector::from(vec![1.0]), false),
                    )
                    .is_empty()
                {
                    let (new_memory, _, _) = self.create_empty_sub_graph();
                    self.structure.add_edge(*node, new_memory, ());
                }
            }
            // If the node is not part of the RC, we do nothing
            None => (),
        }
    }

    fn extend_up(&mut self) {
        // Collect top most memory nodes, which are the only ones that have no incoming edges
        let top_most_memory_nodes: Vec<NodeIndex> = self
            .structure
            .node_indices()
            .filter(|&node| self.structure.edges_directed(node, Incoming).count() == 0)
            .collect();

        // Add a new simple Memory -> Sum -> Product structure above and connect
        for memory_node in &top_most_memory_nodes {
            let (_, _, new_product) = self.create_empty_sub_graph();
            self.structure.add_edge(new_product, *memory_node, ());
        }
    }

    fn distribute(&mut self, leaf: &NodeIndex, expand: bool) -> Option<bool> {
        // Result is None if operation could not be done
        // True if distribute was applied
        // False if leaf hit limit of graph and no expansion was allowed
        let mut result = None;

        // Find all relevant products
        let products_containing_leaf = self.get_parents(leaf);
        assert!(
            !products_containing_leaf.is_empty(),
            "Leaf was found without any parent product nodes!"
        );

        // Apply distributive law
        for product in &products_containing_leaf {
            // If there is a connected sum node, push leaf into all of its products
            let sum_children =
                self.filter_nodes_by_type(&self.get_children(product), &NodeType::Sum);

            if sum_children.is_empty() {
                // If there is no sum node, check if there is a memory node instead
                let mem_child = self.filter_nodes_by_type(
                    &self.get_children(product),
                    &NodeType::Memory(Vector::from(vec![1.0]), false),
                );
                debug_assert!(
                    mem_child.len() < 2,
                    "A product had more than one memory child!"
                );

                // If there is no memory node and we are allowed to create new nodes, make a new sum product
                if mem_child.is_empty() && expand {
                    // Remove old edges pointing at leaf
                    self.remove_incoming_edges(leaf);

                    // Create new nodes
                    let new_sum = self.structure.add_node(NodeType::Sum);
                    let new_product = self.structure.add_node(NodeType::Product);

                    // Connect everything
                    self.structure.add_edge(*product, new_sum, ());
                    self.structure.add_edge(new_sum, new_product, ());
                    self.structure.add_edge(new_product, *leaf, ());

                    // Memorize this product node
                    self.products.push(new_product);

                    // Remove old edge pointing at leaf
                    self.structure
                        .remove_edge(self.structure.find_edge(*product, *leaf).unwrap());

                    result = Some(true);
                } else {
                    // In this case, the drop method needs to be used to push into memory/lower frequency band
                    result = Some(false);
                }
            } else {
                // Push leaf into inner products of referenced sums
                for sum_child in sum_children {
                    for inner_product in self.get_children(&sum_child) {
                        self.structure.add_edge(inner_product, *leaf, ());
                    }
                }

                // Remove old edge pointing at leaf
                self.structure
                    .remove_edge(self.structure.find_edge(*product, *leaf).unwrap());

                result = Some(true);
            }
        }

        result
    }

    fn collect(&mut self, leaf: &NodeIndex) -> Option<bool> {
        // Result is None if operation could not be done
        // True if collect was applied
        // False if leaf hit limit of graph
        let mut result = None;

        // Find all relevant products
        let products_containing_leaf = self.get_parents(leaf);
        debug_assert!(
            !products_containing_leaf.is_empty(),
            "Leaf was found without any parent product nodes!"
        );

        // Apply reverse distributive law
        for product in &products_containing_leaf {
            let parent_sums = self.get_parents(product);
            debug_assert!(
                !parent_sums.is_empty(),
                "Found products withou sum nodes as parents in RC!"
            );

            // We need to go up the graph by two steps
            for parent_sum in &parent_sums {
                // Check the parent of the parent
                // If it is a product, we can push the leaf up
                let grandparents = self.get_parents(parent_sum);
                if self.check_node_type(&grandparents[0], &NodeType::Product) {
                    // Go into all products that multiply with the original sum over the leaf's parent
                    for grandparent in &grandparents {
                        self.structure.add_edge(*grandparent, *leaf, ());
                    }

                    // Remove old edge pointing at leaf
                    self.structure
                        .remove_edge(self.structure.find_edge(*product, *leaf).unwrap());
                    result = Some(true);
                }
                // Else, there must be a memory node and we need to apply the lift method instead
                else {
                    result = Some(false);
                }
            }
        }

        result
    }

    pub fn value(&self, node: &NodeIndex) -> Vector {
        match self
            .structure
            .node_weight(*node)
            .expect("Node was not found within RC!")
        {
            NodeType::Leaf(value) => return Vector::from(vec![*value as f64]),
            NodeType::Product => {
                let mut result = Vector::from(vec![1.0]);

                let values: Vec<Vector> = self
                    .get_children(node)
                    .par_iter()
                    .map(|child| self.value(&child))
                    .collect();

                for value in &values {
                    result *= value;
                }

                return result;
            }
            NodeType::Sum => {
                let mut result = Vector::from(vec![0.0]);

                let values: Vec<Vector> = self
                    .get_children(node)
                    .par_iter()
                    .map(|child| self.value(&child))
                    .collect();
                for value in &values {
                    result += value;
                }

                return result;
            }
            NodeType::Memory(value, _) => return value.clone(),
        }
    }

    pub fn update(&mut self, node: &NodeIndex) {
        match self
            .structure
            .node_weight(*node)
            .expect("Node was not found within RC!")
        {
            NodeType::Memory(_, updated) => match updated {
                true => (),
                false => {
                    let value = self.value(&self.get_children(node)[0]);
                    self.structure[*node] = NodeType::Memory(value.clone(), true);
                }
            },
            _ => (),
        }
    }

    pub fn lift(&mut self, index: usize) -> bool {
        // Find leaf node in graph
        let leaf = self
            .find_leaf(index)
            .expect("Leaf could not be found in RC!");

        // Distribute leaf until it reaches the top of its current memorized sub-graph
        loop {
            match self.collect(&leaf) {
                Some(true) => continue, // Could collect leaf and move a level up
                Some(false) => break,   // Could not move another level up
                None => return false,   // Some problem occured, e.g., misconfigured graph
            }
        }

        // Find all relevant products
        let products_containing_leaf = self.get_parents(&leaf);
        assert!(
            !products_containing_leaf.is_empty(),
            "Leaf was found without any parent product nodes!"
        );

        for product in &products_containing_leaf {
            // Get the set of products without this leaf
            let siblings = self.get_siblings(product);
            let non_leaf_siblings: Vec<&NodeIndex> = siblings
                .iter()
                .filter(|sibling| self.structure.find_edge(**sibling, leaf).is_none())
                .collect();

            //
            if !non_leaf_siblings.is_empty() {}
            let non_leaf_sum = self.structure.add_node(NodeType::Sum);

            let memory_nodes = self.find_next_ancestors_by_type(
                product,
                &NodeType::Memory(Vector::from(vec![1.0]), false),
            );

            // Lift the leaf above this sub-graphs memory node
            for memory_node in &memory_nodes {
                // If there is nothing above the memory node, we have to create a new sub-graph above and add the leaf there
                if self
                    .structure
                    .edges_directed(*memory_node, Incoming)
                    .peekable()
                    .peek()
                    .is_none()
                {
                    let (_, _, new_product) = self.create_empty_sub_graph();
                    self.structure.add_edge(new_product, *memory_node, ());
                    self.structure.add_edge(new_product, leaf, ());
                }
                // There is a sub-graph above that we can attach the leaf to
                else {
                    // TODO: This is wrong if neigbouring products do not contain this leaf
                    let parent_products = self.get_parents(memory_node);
                    for parent_product in &parent_products {
                        // Check that the leaf was not already added through a different path
                        if self.structure.find_edge(*parent_product, leaf).is_none() {
                            self.structure.add_edge(*parent_product, leaf, ());
                        }
                    }
                }

                // Remove old edge pointing at leaf
                self.structure
                    .remove_edge(self.structure.find_edge(*product, leaf).unwrap());
            }
        }

        true
    }

    pub fn drop(&mut self, index: usize) -> bool {
        // Find leaf node in graph
        let leaf = self
            .find_leaf(index)
            .expect("Leaf could not be found in RC!");

        // Distribute leaf until it reaches the bottom of its current memorized sub-graph
        loop {
            match self.distribute(&leaf, false) {
                Some(true) => continue, // Could distribute leaf and move a level down
                Some(false) => break,   // Could not move another level down
                None => return false,   // Some problem occured, e.g., misconfigured graph
            }
        }

        // Find all relevant products
        let products_containing_leaf = self.get_parents(&leaf);
        assert!(
            !products_containing_leaf.is_empty(),
            "Leaf was found without any parent product nodes!"
        );

        for product in &products_containing_leaf {
            // There can not be any sums, only memory nodes, since otherwise distribute would have pushed the leaf further down
            let memory_nodes = self.filter_nodes_by_type(
                &self.get_children(product),
                &NodeType::Memory(Vector::from(vec![1.0]), false),
            );

            // If there is no sub-graph to drop the leaf into, we create a new one
            if memory_nodes.is_empty() {
                let (new_memory, _, new_product) = self.create_empty_sub_graph();
                self.structure.add_edge(*product, new_memory, ());
                self.structure.add_edge(new_product, leaf, ());

                // Remove old edge pointing at leaf
                self.structure
                    .remove_edge(self.structure.find_edge(*product, leaf).unwrap());
            }
            // There is at least one sub-graph that we can drop the leaf into
            else {
                for memory_node in &memory_nodes {
                    for sum_node in &self.get_children(memory_node) {
                        for product_node in &self.get_children(sum_node) {
                            // Check that the leaf was not already added through a different path
                            if self.structure.find_edge(*product_node, leaf).is_none() {
                                self.structure.add_edge(*product_node, leaf, ());
                            }
                        }
                    }
                }
            }
        }

        true
    }

    pub fn prune(&mut self) {
        // Remove all nodes with no outgoing edges until convergence
        loop {
            // Collect nodes without outgoing edges that are not leafs
            let nodes_to_remove: Vec<NodeIndex> = self
                .structure
                .node_indices()
                .filter(|&node| {
                    !self.check_node_type(&node, &NodeType::Leaf(0))
                        && self.structure.edges_directed(node, Outgoing).count() == 0
                })
                .collect();

            // Check if we are done
            if nodes_to_remove.is_empty() {
                break;
            }

            // Remove the nodes if necessary and repeat
            for node in nodes_to_remove {
                self.structure.remove_node(node);
                if self.check_node_type(&node, &NodeType::Product) {
                    self.products.retain(|product| *product != node);
                }
            }
        }
    }

    pub fn to_dot_text(&self) -> String {
        let mut dot = String::new();

        // Start the DOT graph
        dot.push_str("digraph ReactiveCircuit {\n");

        // Iterate over the nodes
        for node in self.structure.node_indices() {
            let node_type = &self.structure[node];
            let node_label = match node_type {
                NodeType::Memory(vector, updated) => format!("Memory({:?}, {:?})", vector, updated),
                NodeType::Sum => format!("Sum"),
                NodeType::Product => "Product".to_string(),
                NodeType::Leaf(index) => format!("Leaf({})", index),
            };
            dot.push_str(&format!(
                "    {} [label=\"{}\"];\n",
                node.index(),
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

    pub fn to_dot(&self, filename: &str) -> std::io::Result<()> {
        // Translate graph into DOT text
        let dot = self.to_dot_text();

        // Write to disk
        let mut file = File::create(filename)?;
        file.write_all(dot.as_bytes())?;
        Ok(())
    }

    pub fn to_svg(&self, filename: &str) -> std::io::Result<()> {
        // Translate graph into DOT text and write to disk
        self.to_dot(filename);

        // Compile into SVG using graphviz
        let svg_text = Command::new("dot")
            .args(["-Tsvg", filename])
            .output()
            .expect("Failed to run graphviz!");

        // Pass stdout into new file with SVG content
        let mut file = File::create(filename)?;
        file.write_all(&svg_text.stdout)?;
        file.sync_all()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use super::ReactiveCircuit;

    #[test]
    fn test_rc() -> std::io::Result<()> {
        let mut rc = ReactiveCircuit::from_sum_product(&vec![vec![0, 1, 2], vec![1, 3]]);

        rc.to_svg("original.svg")?;
        rc.lift(1);
        rc.to_svg("lifted_1.svg")?;
        rc.drop(2);
        rc.to_svg("drop_2.svg")?;

        Ok(())
    }
}
