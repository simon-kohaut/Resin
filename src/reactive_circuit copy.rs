use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::nodes::{SharedLeaf, operator};
use crate::nodes::SharedOperator;
use crate::nodes::{add_leaf, add_operator, product_node, sum_node};

pub struct Layer {
    roots: Vec<SharedOperator>,
    leafs: Vec<SharedLeaf>,
}

pub struct ReactiveCircuit {
    pub root: SharedOperator,
    leafs: Vec<Vec<SharedLeaf>>,
    layers: Vec<Layer>,
}

impl ReactiveCircuit {
    pub fn new() -> Self {
        Self {
            leafs: Vec::new(),
            root: sum_node(),
            layers: Vec::new(),
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

    pub fn lift(&mut self, leaf: &SharedLeaf) {
        for layer in &mut self.layers {
            if layer.contains(&leaf) {
                layer.remove(&leaf);
                layer.prune();
            }
        }

        // let mut leaf_layer = &mut self.layers[layer_index];
        // let mut layer_above = &mut self.layers[layer_index - 1];

        // leaf_layer.leafs.retain(|l| !Arc::ptr_eq(l, leaf));
        // layer_above.leafs.push(leaf.clone());
    }
}

impl Layer {
    pub fn leaf_containers(&self, leaf: &SharedLeaf) -> Vec<SharedOperator> {
        let mut containers = Vec::new();
        for root in &self.roots {
            if root.lock().unwrap().contains(leaf) {
                containers.push(root.clone());
            }
        }

        containers
    }

    pub fn contains(&self, leaf: &SharedLeaf) -> bool {
        for own_leaf in &self.leafs {
            if Arc::ptr_eq(&own_leaf, &leaf) {
                return true;
            }
        }

        false
    }

    pub fn remove(&mut self, leaf: &SharedLeaf) {
        self.leafs.retain(|l| Arc::ptr_eq(&l, &leaf));

        for root in &mut self.roots {
            root.lock().unwrap().remove(&leaf);
        }
    }

    pub fn prune(&mut self) {
        for root in &mut self.roots {
            root.lock().unwrap().prune();
        }
    }
}


pub struct RC {
    root: SharedOperator,
    top_layer: Option<Box<RC>>,
    sub_layers: Vec<Arc<Mutex<RC>>>,
}

impl RC {
    pub fn new() -> Self {
        Self {
            root: sum_node(),
            top_layer: None,
            sub_layers: Vec::new(),
        }
    }

    pub fn value(&self) -> f64 {
        let mut root_guard = self.root.lock().unwrap();
        root_guard.update();
        root_guard.value
    }

    pub fn add_product(&mut self, leafs: Vec<SharedLeaf>) {
        let product = product_node();
        
        for leaf in leafs {
            add_leaf(leaf, product.clone());
        }

        add_operator(product, self.root.clone());
    }
}

struct SubTree {
    root: SharedOperator
}

impl SubTree {
    pub fn new() -> Self {
        Self {
            root: sum_node(),
        }
    }

    pub fn value(&self) -> f64 {
        let mut root_guard = self.root.lock().unwrap();
        root_guard.update();
        root_guard.value
    }

    pub fn add_product(&mut self, leafs: Vec<SharedLeaf>) {
        let product = product_node();
        
        for leaf in leafs {
            add_leaf(leaf, product.clone());
        }

        add_operator(product, self.root.clone());
    }
}

pub fn lift(reactive_circuit: &mut SharedOperator, leaf: SharedLeaf) {
    let mut lift_leaf = false;
    
    for product in &reactive_circuit.lock().unwrap().operators {
        let guard = product.lock().unwrap();
        if guard.leafs_contain(&leaf) {
            guard.remove_from_leafs(&leaf);
            lift_leaf = true;
        }
    }

    if lift_leaf {
        return leaf;
    }
}

pub fn sum_products(products: Vec<Vec<SharedLeaf>>) -> ReactiveCircuit {
    let circuit = ReactiveCircuit::new();

    for product in products {
        circuit.add_product(product);
    }

    circuit
}
