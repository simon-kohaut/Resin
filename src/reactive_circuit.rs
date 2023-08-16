// Standard library
use std::{
    str::FromStr,
    sync::{Arc, Mutex},
};

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

    pub fn lift(&self, leafs: Vec<SharedLeaf>) -> ReactiveCircuit {
        let mut lifted_circuit = self.copy();
        for leaf in &leafs {
            lifted_circuit = lift(&lifted_circuit, leaf.clone());
        }

        prune(&lifted_circuit).unwrap()
    }

    pub fn drop(&self, leafs: Vec<SharedLeaf>) -> ReactiveCircuit {
        let mut lifted_circuit = self.copy();
        for leaf in &leafs {
            lifted_circuit = drop(&lifted_circuit, leaf.clone());
        }

        prune(&lifted_circuit).unwrap()
    }

    pub fn to_dot_file(&self, index: Option<i32>) -> String {
        let mut dot_text = String::new();

        let mut circuit_index = 0;
        if index.is_some() {
            circuit_index = index.unwrap();
        }

        dot_text += &String::from_str(&format!("rc{index} [shape=box, label=\"RC{index}\"]\n", index=circuit_index)).unwrap();
        dot_text += &String::from_str(&format!("s{} [label=\"+\"]\n", circuit_index)).unwrap();
        dot_text += &String::from_str(&format!("rc{index} -> s{index}\n", index=circuit_index)).unwrap();

        let mut model_index = 0;
        for model in &self.models {
            // Add product node for this model
            dot_text += &String::from_str(&format!(
                "p{}{} [label=\"*\"]\n",
                circuit_index, model_index
            ))
            .unwrap();

            // Add connection of sum-root to this model's product
            dot_text += &String::from_str(&format!(
                "s{} -> p{}{}\n",
                circuit_index, circuit_index, model_index
            ))
            .unwrap();

            // Add leaf node connections
            for leaf in &model.leafs {
                dot_text += &String::from_str(&format!(
                    "p{}{} -> {}\n",
                    circuit_index,
                    model_index,
                    leaf.lock().unwrap().name
                ))
                .unwrap();
            }

            if model.circuit.is_some() {
                dot_text += &String::from_str(&format!(
                    "p{}{} -> rc{}\n",
                    circuit_index,
                    model_index,
                    circuit_index + 1
                ))
                .unwrap();

                dot_text += &model
                    .circuit
                    .as_ref()
                    .unwrap()
                    .to_dot_file(Some(circuit_index + 1));
            }

            model_index += 1;
        }

        dot_text
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
        // A new root with two models, one containing the relevant leaf and a circuit
        // of all models with that leaf, one with all models that do not contain the leaf
        let mut root_circuit = ReactiveCircuit::new();
        let mut non_leaf_circuit = ReactiveCircuit::new();
        let mut leaf_circuit = ReactiveCircuit::new();

        for model in &mut updated_circuit.models {
            if model.contains(leaf.clone()) {
                model.remove(leaf.clone());
                leaf_circuit.add_model(model.copy());
            } else {
                non_leaf_circuit.add_model(model.copy());
            }
        }

        root_circuit.add_model(Model::new(Vec::new(), Some(non_leaf_circuit)));
        root_circuit.add_model(Model::new(vec![leaf.clone()], Some(leaf_circuit)));
        updated_circuit = root_circuit;
    } else {
        for model in &mut updated_circuit.models {
            if model.circuit.is_some() {
                if model.circuit.as_ref().unwrap().contains(leaf.clone()) {
                    model.append(leaf.clone());
                    model.circuit.as_mut().unwrap().remove(leaf.clone());
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

    // println!("#models: {}, #leafs in model0: {}", updated_circuit.models.len(), updated_circuit.models[0].leafs.len());
    if updated_circuit.models.len() == 1 && updated_circuit.models[0].leafs.len() == 0 {
        let lonesome_circuit = updated_circuit.models[0].circuit.as_ref().unwrap();
        updated_circuit = lonesome_circuit.copy();
    }

    // Merge all underlying circuits into one if this one does not have any leafs
    let mut contains_leafs = false;
    for model in &updated_circuit.models {
        if model.leafs.len() > 0 {
            contains_leafs = true;
        }
    }

    if !contains_leafs {
        let mut merge_circuit = ReactiveCircuit::new();
        for model in &updated_circuit.models {
            for inner_model in &model.circuit.as_ref().unwrap().models {
                merge_circuit.add_model(inner_model.copy());
            }
        }
        updated_circuit = merge_circuit;
    }

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
                Some(model_circuit) => {
                    if model.leafs.len() == 0 {
                        write!(f, "{}", model_circuit)?
                    } else {
                        write!(f, " * {}", model_circuit)?
                    }
                }
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
