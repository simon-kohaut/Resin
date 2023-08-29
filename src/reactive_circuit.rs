// Standard library
use std::{
    fs::File,
    io::prelude::*,
    process::Command,
    str::FromStr,
    sync::{Arc, Mutex},
};

// Resin
use crate::nodes::SharedLeaf;

#[derive(Debug)]
pub struct ReactiveCircuit {
    pub models: Vec<Model>,
    valid: bool,
    layer: i32,
}

#[derive(Debug)]
pub struct Model {
    pub leafs: Vec<SharedLeaf>,
    pub circuit: Option<ReactiveCircuit>,
}

pub type SharedModel = Arc<Mutex<Model>>;
pub type SharedReactiveCircuit = Arc<Mutex<ReactiveCircuit>>;

impl ReactiveCircuit {
    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            valid: false,
            layer: 0,
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
        copy.layer = self.layer;
        copy
    }

    pub fn lift(&self, leafs: Vec<SharedLeaf>) -> ReactiveCircuit {
        let mut lift_circuit = self.copy();
        for leaf in &leafs {
            lift_circuit = lift(&lift_circuit, leaf.clone());
        }

        prune(&lift_circuit).unwrap()
    }

    pub fn drop(&self, leafs: Vec<SharedLeaf>) -> ReactiveCircuit {
        let mut drop_circuit = self.copy();
        for leaf in &leafs {
            drop_circuit = drop(&drop_circuit, leaf.clone());
        }

        prune(&drop_circuit).unwrap()
    }

    pub fn to_svg(&self, path: String) -> std::io::Result<()> {
        let mut dot_text = String::from_str("strict digraph {\nnode [shape=circle]\n").unwrap();
        dot_text += &self.to_dot_file(&mut 0);
        dot_text += "}";

        let dot_path = path.clone() + ".dot";
        let mut f = File::create(&dot_path)?;
        f.write_all(dot_text.as_bytes())?;
        f.sync_all()?;

        let svg_text = Command::new("dot")
            .args(["-Tsvg", &dot_path])
            .output()
            .expect("Failed to run graphviz!");

        let svg_path = path.clone() + ".svg";
        let mut f = File::create(svg_path)?;
        f.write_all(&svg_text.stdout)?;
        f.sync_all()?;

        Ok(())
    }

    pub fn to_dot_file(&self, index: &mut i32) -> String {
        let mut dot_text = String::new();

        let circuit_index = index.clone();

        dot_text += &String::from_str(&format!(
            "rc{index} [shape=square, label=\"RC{index} - {layer}\"]\n",
            index = circuit_index,
            layer = self.layer
        ))
        .unwrap();
        dot_text += &String::from_str(&format!("s{} [label=\"+\"]\n", circuit_index)).unwrap();
        dot_text +=
            &String::from_str(&format!("rc{index} -> s{index}\n", index = circuit_index)).unwrap();

        let mut model_index = 0;
        // let mut sub_circuit_index = circuit_index + 1;
        for model in &self.models {
            // Add product node for this model
            dot_text += &String::from_str(&format!(
                "p{circuit}{model} [label=\"&times;\"]\n",
                circuit = circuit_index,
                model = model_index
            ))
            .unwrap();

            // Add connection of sum-root to this model's product
            dot_text += &String::from_str(&format!(
                "s{circuit} -> p{circuit}{model}\n",
                circuit = circuit_index,
                model = model_index
            ))
            .unwrap();

            // Add leaf node connections
            for leaf in &model.leafs {
                dot_text += &String::from_str(&format!(
                    "p{circuit}{model} -> {leaf_name}\n",
                    circuit = circuit_index,
                    model = model_index,
                    leaf_name = leaf.lock().unwrap().name,
                ))
                .unwrap();
            }

            if model.circuit.is_some() {
                *index += 1;

                dot_text += &String::from_str(&format!(
                    "p{circuit}{model}-> rc{next_circuit}\n",
                    circuit = circuit_index,
                    model = model_index,
                    next_circuit = index
                ))
                .unwrap();

                dot_text += &model.circuit.as_ref().unwrap().to_dot_file(index);
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

    pub fn empty(&mut self) {
        self.leafs = Vec::new();
        self.circuit = None;
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

        root_circuit.layer = updated_circuit.layer;
        leaf_circuit.layer = updated_circuit.layer + 1;
        non_leaf_circuit.layer = updated_circuit.layer + 1;

        root_circuit.add_model(Model::new(Vec::new(), Some(non_leaf_circuit)));
        root_circuit.add_model(Model::new(vec![leaf.clone()], Some(leaf_circuit)));
        updated_circuit = root_circuit;
    } else {
        let mut non_leaf_circuit = ReactiveCircuit::new();
        non_leaf_circuit.layer = updated_circuit.layer + 1;
        for model in &mut updated_circuit.models {
            if model.circuit.is_some() {
                if model.circuit.as_ref().unwrap().contains(leaf.clone()) {
                    model.append(leaf.clone());

                    for inner_model in &mut model.circuit.as_mut().unwrap().models {
                        if !inner_model.contains(leaf.clone()) {
                            non_leaf_circuit.add_model(inner_model.copy());
                            inner_model.empty();
                        }
                    }

                    model.circuit.as_mut().unwrap().remove(leaf.clone());
                } else {
                    model.circuit = Some(lift(&model.circuit.as_ref().unwrap(), leaf.clone()));
                }
            }
        }
        updated_circuit.add_model(Model::new(Vec::new(), Some(non_leaf_circuit.copy())));
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
                            layer: updated_circuit.layer + 1,
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

    // Remove this circuit if its only model is a forwarding of another circuit
    // i.e. unneccessary indirection
    if updated_circuit.models.len() == 1
        && updated_circuit.models[0].leafs.len() == 0
        && updated_circuit.layer - updated_circuit.models[0].circuit.as_ref().unwrap().layer > 1
    {
        let lonesome_circuit = updated_circuit.models[0].circuit.as_ref().unwrap();
        updated_circuit = lonesome_circuit.copy();
    }

    // Merge all underlying circuits into one if this one does not have any leafs
    // let mut contains_leafs = false;
    // for model in &updated_circuit.models {
    //     if model.leafs.len() > 0 {
    //         contains_leafs = true;
    //     }
    // }

    // if !contains_leafs {
    //     let mut merge_circuit = ReactiveCircuit::new();
    //     for model in &updated_circuit.models {
    //         for inner_model in &model.circuit.as_ref().unwrap().models {
    //             merge_circuit.add_model(inner_model.copy());
    //         }
    //     }
    //     merge_circuit.layer = updated_circuit.layer;
    //     updated_circuit = merge_circuit;
    // }

    Some(updated_circuit)
}

pub fn fix(circuit: &ReactiveCircuit) -> ReactiveCircuit {
    let mut fixed_circuit = circuit.copy();

    for model in &mut fixed_circuit.models {
        if model.circuit.is_some() {
            let sub_circuit = model.circuit.as_ref().unwrap();
            if sub_circuit.layer - circuit.layer > 1 {
                let mut buffer = ReactiveCircuit::new();
                buffer.layer = circuit.layer + 1;
                buffer.add_model(Model::new(Vec::new(), Some(sub_circuit.copy())));
                model.circuit = Some(buffer);
            }
        }
    }

    fixed_circuit
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
