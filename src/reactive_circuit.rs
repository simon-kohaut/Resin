// Standard library
use std::sync::{Arc, Mutex};

// Resin
use crate::nodes::SharedLeaf;


pub struct ReactiveCircuit {
    models: Vec<Model>,
    valid: bool
}

pub struct Model {
    leafs: Vec<SharedLeaf>,
    circuit: Option<ReactiveCircuit>
}

impl ReactiveCircuit {

    pub fn new() -> Self {
        Self { models: Vec::new(), valid: false }
    }

    pub fn add_model(&mut self, model: Model) {
        self.models.push(model);
    }

    pub fn value(&self) -> f64 {
        let mut sum = 0.0; 

        for model in &self.models {
            sum += model.value();
        }

        sum
    }

    pub fn remove(&mut self, leaf: SharedLeaf) {
        for model in &mut self.models {
            model.remove(leaf.clone());
        }
    }

}

impl Model {

    pub fn new(leafs: Vec<SharedLeaf>, circuit: Option<ReactiveCircuit>) -> Self{
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

    // Write interface
    pub fn append(&mut self, leaf: SharedLeaf) {
        self.leafs.push(leaf.clone());
    }

    pub fn remove(&mut self, leaf: SharedLeaf) {
        self.leafs.retain(|l| Arc::ptr_eq(&l, &leaf));
    }

}

pub fn drop(circuit: &mut ReactiveCircuit, leaf: SharedLeaf) {
    for model in &mut circuit.models {
        if model.contains(leaf.clone()) {
            model.remove(leaf.clone());
            match model.circuit {
                Some(ref mut model_circuit) => {
                    for model in &mut model_circuit.models {
                        model.append(leaf.clone());
                    }
                },
                None => {
                    let mut new_circuit = ReactiveCircuit::new();
                    new_circuit.add_model(Model::new(vec![leaf.clone()], None));
                    model.circuit = Some(new_circuit);
                }
            }
        }
    }
}