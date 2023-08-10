// Standard library
use std::sync::{Arc, Mutex};

// Resin
use crate::nodes::SharedLeaf;

#[derive(Debug)]
pub struct ReactiveCircuit {
    models: Vec<SharedModel>,
    valid: bool,
    parent: Option<SharedModel>,
}

#[derive(Debug)]
pub struct Model {
    leafs: Vec<SharedLeaf>,
    circuit: Option<SharedReactiveCircuit>,
}

pub type SharedModel = Arc<Mutex<Model>>;
pub type SharedReactiveCircuit = Arc<Mutex<ReactiveCircuit>>;

impl ReactiveCircuit {
    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            valid: false,
            parent: None,
        }
    }

    pub fn add_model(&mut self, model: SharedModel) {
        self.models.push(model.clone());
    }

    pub fn value(&self) -> f64 {
        let mut sum = 0.0;

        for model in &self.models {
            sum += model.lock().unwrap().value();
        }

        sum
    }

    pub fn remove(&mut self, leaf: SharedLeaf) {
        for model in &mut self.models {
            model.lock().unwrap().remove(leaf.clone());
        }
    }
}

impl Model {
    pub fn new(leafs: Vec<SharedLeaf>, circuit: Option<SharedReactiveCircuit>) -> Self {
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
            Some(circuit) => product *= circuit.lock().unwrap().value(),
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
        self.leafs.retain(|l| !Arc::ptr_eq(&l, &leaf));
    }
}

pub fn drop(circuit: SharedReactiveCircuit, leaf: SharedLeaf) {
    let mut contained_leaf = false;
    for model in &circuit.lock().unwrap().models {
        let mut model_guard = model.lock().unwrap();
        if model_guard.contains(leaf.clone()) {
            model_guard.remove(leaf.clone());
            contained_leaf = true;

            match &model_guard.circuit {
                Some(model_circuit) => {
                    for circuit_model in &model_circuit.lock().unwrap().models {
                        circuit_model.lock().unwrap().append(leaf.clone());
                    }
                }
                None => {
                    model_guard.circuit = Some(Arc::new(Mutex::new(ReactiveCircuit {
                        models: vec![Arc::new(Mutex::new(Model::new(vec![leaf.clone()], None)))],
                        parent: Some(model.clone()),
                        valid: false,
                    })));
                }
            }
        }
    }

    if !contained_leaf {
        for model in &circuit.lock().unwrap().models {
            let model_guard = model.lock().unwrap();
            match &model_guard.circuit {
                Some(model_circuit) => drop(model_circuit.clone(), leaf.clone()),
                None => (),
            }
        }
    }
}

pub fn lift(circuit: SharedReactiveCircuit, leaf: SharedLeaf) {
    let mut contained_leaf = false;
    for model in &circuit.lock().unwrap().models {
        let mut model_guard = model.lock().unwrap();
        if model_guard.contains(leaf.clone()) {
            model_guard.remove(leaf.clone());
            contained_leaf = true;
        }
    }

    if contained_leaf {
        let mut circuit_guard = circuit.lock().unwrap();
        match &circuit_guard.parent {
            Some(circuit_parent) => circuit_parent.lock().unwrap().append(leaf.clone()),
            None => {
                let new_circuit = ReactiveCircuit {
                    models: vec![Arc::new(Mutex::new(Model::new(
                        vec![leaf],
                        Some(circuit.clone()),
                    )))],
                    valid: true,
                    parent: None,
                };
                *circuit_guard = new_circuit;
            }
        }
    } else {
        for model in &mut circuit.lock().unwrap().models {
            let model_guard = model.lock().unwrap();
            match &model_guard.circuit {
                Some(model_circuit) => lift(model_circuit.clone(), leaf.clone()),
                None => (),
            }
        }
    }
}
