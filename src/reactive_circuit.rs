use crate::nodes::SharedLeaf;
use crate::nodes::SharedOperator;
use crate::nodes::{add_leaf, add_operator, product_node, sum_node};
use itertools::Itertools;

pub struct ReactiveCircuit {
    pub leafs: Vec<Vec<SharedLeaf>>,
    pub root: SharedOperator,
}

impl ReactiveCircuit {
    pub fn new() -> Self {
        Self {
            leafs: Vec::new(),
            root: sum_node(),
        }
    }

    pub fn from_worlds(worlds: Vec<Vec<SharedLeaf>>) -> Self {
        let circuit = Self::new();

        for world in worlds {
            circuit.add_world(world);
        }

        circuit
    }

    pub fn value(&self) -> f64 {
        let mut root_guard = self.root.lock().unwrap();
        root_guard.update();
        root_guard.value
    }

    pub fn remove(&self, leaf: &SharedLeaf) {
        let mut root_guard = self.root.lock().unwrap();
        root_guard.remove(&leaf);
        root_guard.invalidate();
    }

    pub fn add_world(&self, world: Vec<SharedLeaf>) {
        let product = product_node();
        for leaf in world {
            add_leaf(leaf.clone(), product.clone());
        }
        add_operator(product.clone(), self.root.clone());
    }

    pub fn power_set(leafs: &[SharedLeaf]) -> Vec<Vec<&SharedLeaf>> {
        let mut power_set = Vec::new();
        for i in 0..leafs.len() + 1 {
            for set in leafs.iter().combinations(i) {
                power_set.push(set);
            }
        }
        power_set
    }
}
