// Standard library
use std::sync::{Arc, Mutex};

// Resin
use crate::nodes::SharedLeaf;

#[derive(Debug)]
pub struct ReactiveCircuit {
    models: Vec<Model>,
    valid: bool,
}

#[derive(Debug)]
pub struct Model {
    leafs: Vec<SharedLeaf>,
    circuit: Option<ReactiveCircuit>,
}

pub type SharedModel = Arc<Mutex<Model>>;
pub type SharedReactiveCircuit = Arc<Mutex<ReactiveCircuit>>;

impl ReactiveCircuit {
    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            valid: false,
        }
    }

    // Read interface
    pub fn value(&self) -> f64 {
        let mut sum = 0.0;

        for model in &self.models {
            sum += model.value();
        }

        sum
    }

    pub fn contains(&self, leaf: SharedLeaf) -> bool {
        for model in &self.models {
            if model.contains(leaf.clone()) {
                return true;
            }
        }
        false
    }

    pub fn copy(&self) -> ReactiveCircuit {
        let mut copy = ReactiveCircuit::new();
        for model in &self.models {
            copy.add_model(model.copy());
        }
        copy
    }

    // Write interface
    pub fn remove(&mut self, leaf: SharedLeaf) {
        for model in &mut self.models {
            model.remove(leaf.clone());
        }
    }

    pub fn add_model(&mut self, model: Model) {
        self.models.push(model);
    }
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
}

pub fn lift(circuit: &ReactiveCircuit, leaf: SharedLeaf) -> ReactiveCircuit {
    let mut updated_circuit = circuit.copy();

    // Assume we will only visit a circuit containing this leaf if
    // it is the root circuit. Otherwise, we remove the leaf beforehand to
    // not require a reference to the parent circuit
    if updated_circuit.contains(leaf.clone()) {
        for model in &mut updated_circuit.models {
            model.remove(leaf.clone());
        }

        let mut new_root_circuit = ReactiveCircuit::new();
        new_root_circuit.add_model(Model::new(vec![leaf.clone()], Some(updated_circuit.copy())));
        updated_circuit = new_root_circuit;
    } else {
        for model in &mut updated_circuit.models {
            if model.circuit.is_some() {
                if model.circuit.as_ref().unwrap().contains(leaf.clone()) {
                    model.circuit.as_mut().unwrap().remove(leaf.clone());
                    model.append(leaf.clone());
                } else {
                    model.circuit = Some(lift(&model.circuit.as_ref().unwrap(), leaf.clone()));
                }
            }
        }
    }
    updated_circuit
}

pub fn drop(circuit: &ReactiveCircuit, leaf: SharedLeaf) -> ReactiveCircuit {
    let mut updated_circuit = circuit.copy();
    if updated_circuit.contains(leaf.clone()) {
        for model in &mut updated_circuit.models {
            if model.contains(leaf.clone()) {
                model.remove(leaf.clone());

                match &mut model.circuit {
                    Some(model_circuit) => {
                        for circuit_model in &mut model_circuit.models {
                            circuit_model.append(leaf.clone());
                        }
                    }
                    None => {
                        model.circuit = Some(ReactiveCircuit {
                            models: vec![Model::new(vec![leaf.clone()], None)],
                            valid: false,
                        });
                    }
                }
            }
        }
    } else {
        for model in &mut updated_circuit.models {
            if model.circuit.is_some() {
                model.circuit = Some(drop(&model.circuit.as_ref().unwrap(), leaf.clone()));
            }
        }
    }
    updated_circuit
}

pub fn prune(circuit: &ReactiveCircuit) -> Option<ReactiveCircuit> {
    let mut updated_circuit = circuit.copy();

    // Prune underlying circuits
    for model in &mut updated_circuit.models {
        if model.circuit.is_some() {
           model.circuit = prune(&model.circuit.as_ref().unwrap());
        }
    }

    // Remove empty models
    updated_circuit
        .models
        .retain(|m| m.leafs.len() > 0 || m.circuit.is_some());

    // Remove this circuit if it is empty
    if updated_circuit.models.len() == 0 {
        return None;
    }

    // TODO: Merge circuits horizontally where domain of upper circuits are equal

    Some(updated_circuit)
}

impl std::fmt::Display for ReactiveCircuit {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        // Peekable iterate over models of this RC
        let mut model_iterator = self.models.iter().peekable();
        while let Some(model) = model_iterator.next() {
            // Write all leafs as a product (a * b * ...)
            write!(f, "(")?;
            let mut leaf_iterator = model.leafs.iter().peekable();
            while let Some(leaf) = leaf_iterator.next() {
                write!(f, "{}", leaf.lock().unwrap().name)?;
                if !leaf_iterator.peek().is_none() {
                    write!(f, " * ")?;
                }
            }

            // Write next RC within this ones product, i.e., (... * (d * e * ...))
            match &model.circuit {
                Some(model_circuit) => write!(f, " * {}", model_circuit)?,
                None => (),
            }
            write!(f, ")")?;

            // Models next to each other are added together
            if !model_iterator.peek().is_none() {
                write!(f, " + ")?;
            }
        }
        Ok(())
    }
}
