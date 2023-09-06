// Standard library
use std::sync::{Arc, Mutex};

// Resin
use crate::circuit::SharedLeaf;
use crate::circuit::ReactiveCircuit;

pub type SharedModel = Arc<Mutex<Model>>;

#[derive(Debug)]
pub struct Model {
    pub leafs: Vec<SharedLeaf>,
    pub circuit: Option<ReactiveCircuit>,
}

impl Model {
    pub fn new(leafs: Vec<SharedLeaf>, circuit: Option<ReactiveCircuit>) -> Self {
        Self { leafs, circuit }
    }

    // Read interface
    pub fn value(&self) -> f64 {
        let mut product = 1.0;

        for leaf in &self.leafs {
            let leaf_guard = leaf.lock().unwrap();
            product *= leaf_guard.get_value();
        }

        match &self.circuit {
            Some(circuit) => product *= circuit.value(),
            None => (),
        }

        product
    }

    pub fn contains(&self, searched_leaf: SharedLeaf) -> bool {
        for leaf in self.leafs.iter() {
            if Arc::ptr_eq(&leaf, &searched_leaf) {
                return true;
            }
        }

        false
    }

    pub fn copy(&self) -> Model {
        let mut copy = Model::new(vec![], None);

        for leaf in &self.leafs {
            copy.append(leaf.clone());
        }

        match &self.circuit {
            Some(circuit) => copy.circuit = Some(circuit.copy()),
            None => (),
        }

        copy
    }

    // Write interface
    pub fn append(&mut self, leaf: SharedLeaf) {
        self.leafs.push(leaf.clone());
    }

    pub fn remove(&mut self, leaf: SharedLeaf) {
        self.leafs.retain(|l| !Arc::ptr_eq(&l, &leaf));
    }

    pub fn empty(&mut self) {
        self.leafs = Vec::new();
        self.circuit = None;
    }
}
