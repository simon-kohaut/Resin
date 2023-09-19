// Standard library
use std::sync::{Arc, Mutex};

// Resin
use crate::circuit::SharedLeaf;
use crate::circuit::SharedReactiveCircuit;

pub type SharedModel = Arc<Mutex<Model>>;

pub struct Model {
    pub leafs: Vec<SharedLeaf>,
    pub circuit: Option<SharedReactiveCircuit>,
}

impl Model {
    pub fn new(leafs: &[SharedLeaf], circuit: &Option<SharedReactiveCircuit>) -> Self {
        Self {
            leafs: leafs.to_owned(),
            circuit: circuit.clone(),
        }
    }

    // Read interface
    pub fn value(&self) -> f64 {
        let mut product = 1.0;

        for leaf in &self.leafs {
            product *= leaf.lock().unwrap().get_value();
        }

        match &self.circuit {
            Some(circuit) => product *= circuit.lock().unwrap().get_value(),
            None => (),
        }

        product
    }

    pub fn contains(&self, leaf: &SharedLeaf) -> bool {
        for own_leaf in self.leafs.iter() {
            if Arc::ptr_eq(&own_leaf, &leaf) {
                return true;
            }
        }

        false
    }

    pub fn copy(&self) -> Model {
        let mut copy = Model::new(&vec![], &None);

        for leaf in &self.leafs {
            copy.append(&leaf);
        }

        match &self.circuit {
            Some(circuit) => copy.circuit = Some(circuit.clone()),
            None => (),
        }

        copy
    }

    // Write interface
    pub fn append(&mut self, leaf: &SharedLeaf) {
        self.leafs.push(leaf.clone());
    }

    pub fn remove(&mut self, leaf: &SharedLeaf) {
        self.leafs.retain(|l| !Arc::ptr_eq(&l, &leaf));
    }

    pub fn empty(&mut self) {
        self.leafs = Vec::new();
        self.circuit = None;
    }
}
