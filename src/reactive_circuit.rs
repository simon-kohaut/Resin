use crate::nodes::SharedLeaf;
use crate::nodes::SharedOperator;
use crate::nodes::{product_node, sum_node, add_leaf, add_operator};

pub struct ReactiveCircuit {
    pub leafs: Vec<Vec<SharedLeaf>>,
    pub root: SharedOperator
}

impl ReactiveCircuit {
    pub fn new() -> Self {
        Self {
            leafs: Vec::new(),
            root: sum_node(),
        }
    }

    pub fn value(&self) -> f64 {
        let mut root_guard = self.root.lock().unwrap();
        root_guard.update();
        root_guard.value
    }

    // pub fn lift(&self, leaf: SharedLeaf) {
    //     let sum = sum_node();
    //     let product = product_node();
        
    //     add_leaf(leaf.clone(), product_node.clone());
        
    //     if product.contains(leaf) {
    //         product.remove(leaf);
    //         product.set_parent(sum);
    //     }
    // }

    pub fn add_world(&self, world: Vec<SharedLeaf>) {
        let product = product_node();
        for leaf in world {
            add_leaf(leaf.clone(), product.clone());
        }
        add_operator(product.clone(), self.root.clone());
    }
}
