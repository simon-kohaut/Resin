// Standard library
use std::{
    fs::File,
    io::prelude::*,
    process::Command,
    str::FromStr,
    sync::{Arc, Mutex},
};

// Resin
use crate::circuit::SharedLeaf;
use crate::{circuit::Model, frequencies};

pub struct ReactiveCircuit {
    pub models: Vec<Model>,
    pub parent: Option<SharedReactiveCircuit>,
    pub layer: i32,
    value: f64,
    valid: bool,
}

pub type SharedReactiveCircuit = Arc<Mutex<ReactiveCircuit>>;

impl ReactiveCircuit {
    pub fn new(models: Vec<Model>, parent: Option<SharedReactiveCircuit>, layer: i32) -> Self {
        Self {
            models,
            parent,
            layer,
            value: 0.0,
            valid: false,
        }
    }

    pub fn empty_new() -> Self {
        Self {
            models: Vec::new(),
            parent: None,
            layer: 0,
            value: 0.0,
            valid: false,
        }
    }

    pub fn share(&self) -> SharedReactiveCircuit {
        Arc::new(Mutex::new(self.copy()))
    }

    // Read interface
    pub fn contains(&self, leaf: &SharedLeaf) -> bool {
        for model in &self.models {
            if model.contains(leaf) {
                return true;
            }
        }
        false
    }

    pub fn copy(&self) -> ReactiveCircuit {
        let mut copy = ReactiveCircuit::empty_new();
        for model in &self.models {
            copy.models.push(model.copy());
        }
        copy.layer = self.layer;
        copy
    }

    pub fn to_svg(&self, path: &str) -> std::io::Result<()> {
        let mut dot_text = String::from_str("strict digraph {\nnode [shape=circle]\n").unwrap();
        dot_text += &self.get_dot_text(&mut 0);
        dot_text += "}";

        self.to_dot(path)?;

        let svg_text = Command::new("dot")
            .args(["-Tsvg", &path])
            .output()
            .expect("Failed to run graphviz!");

        let mut f = File::create(path)?;
        f.write_all(&svg_text.stdout)?;
        f.sync_all()?;

        Ok(())
    }

    pub fn to_dot(&self, path: &str) -> std::io::Result<()> {
        let mut dot_text = String::from_str("strict digraph {\nnode [shape=circle]\n").unwrap();
        dot_text += &self.get_dot_text(&mut 0);
        dot_text += "}";

        let mut file = File::create(path)?;
        file.write_all(dot_text.as_bytes())?;
        file.sync_all()?;

        Ok(())
    }

    pub fn get_dot_text(&self, index: &mut i32) -> String {
        let mut dot_text = String::new();

        let circuit_index = index.clone();

        let color = if self.valid {
            "color=deepskyblue"
        } else {
            "color=firebrick"
        };

        dot_text += &String::from_str(&format!(
            "rc{index} [shape=square, label=\"RC{index} - {layer}\n{value:.2}\", {color}]\n",
            index = circuit_index,
            layer = self.layer,
            value = self.value,
            color = color
        ))
        .unwrap();
        dot_text += &String::from_str(&format!("s{} [label=\"+\"]\n", circuit_index)).unwrap();
        dot_text += &String::from_str(&format!(
            "rc{index} -> s{index} [{color}]\n",
            index = circuit_index,
            color = color
        ))
        .unwrap();

        let mut model_index = 0;
        // let mut sub_circuit_index = circuit_index + 1;
        for model in &self.models {
            // Add product node for this model
            dot_text += &String::from_str(&format!(
                "p{circuit}{model} [label=\"&times;\", {color}]\n",
                circuit = circuit_index,
                model = model_index,
                color = color
            ))
            .unwrap();

            // Add connection of sum-root to this model's product
            dot_text += &String::from_str(&format!(
                "s{circuit} [{color}]\n",
                circuit = circuit_index,
                color = color
            ))
            .unwrap();
            dot_text += &String::from_str(&format!(
                "s{circuit} -> p{circuit}{model} [{color}]\n",
                circuit = circuit_index,
                model = model_index,
                color = color
            ))
            .unwrap();

            // Add leaf node connections
            for leaf in &model.leafs {
                let name = leaf.lock().unwrap().name.clone();
                let value = leaf.lock().unwrap().get_value();
                let frequency = leaf.lock().unwrap().get_frequency();
                let cluster = leaf.lock().unwrap().get_cluster();
                dot_text += &String::from_str(&format!(
                    "\"{name}\nP = {value:.3}\nf = {frequency:.3}\nC = {cluster}\" [{color}]\n",
                    name = name,
                    color = color,
                    value = value,
                    frequency = frequency,
                    cluster = cluster
                ))
                .unwrap();
                dot_text += &String::from_str(&format!(
                    "p{circuit}{model} -> \"{leaf_name}\" [{color}]\n",
                    circuit = circuit_index,
                    model = model_index,
                    leaf_name = leaf.lock().unwrap().name,
                    color = color
                ))
                .unwrap();
            }

            match &model.circuit {
                Some(model_circuit) => {
                    *index += 1;

                    dot_text += &String::from_str(&format!(
                        "p{circuit}{model}-> rc{next_circuit} [{color}]\n",
                        circuit = circuit_index,
                        model = model_index,
                        next_circuit = index,
                        color = color
                    ))
                    .unwrap();

                    dot_text += &model_circuit.lock().unwrap().get_dot_text(index);
                }
                None => (),
            }

            model_index += 1;
        }

        dot_text
    }

    // Write interface
    pub fn get_value(&mut self) -> (f64, usize) {
        // If already valid, just return the value without operations
        if self.valid {
            return (self.value, 0);
        }

        // Invalid RC needs to recompute its value and operations count
        let (mut sum, mut operations_count) = self.models[0].get_value();

        for model in &self.models[1..] {
            let (model_value, model_operations) = model.get_value();
            sum += model_value;
            operations_count += model_operations + 1; // Account for the addition with +1
        }

        self.value = sum;
        self.valid = true;

        (sum, operations_count)
    }

    pub fn invalidate(&mut self) {
        self.valid = false;
        if self.parent.is_some() {
            self.parent.as_mut().unwrap().lock().unwrap().invalidate();
        }
    }

    pub fn remove(&mut self, leaf: &SharedLeaf) {
        for model in &mut self.models {
            model.remove(leaf);
        }
    }
}

pub fn add_model(
    circuit: &SharedReactiveCircuit,
    leafs: &[SharedLeaf],
    sub_circuit: &Option<SharedReactiveCircuit>,
) {
    let model = Model::new(&leafs, &sub_circuit);
    circuit.lock().unwrap().models.push(model);

    for leaf in leafs {
        leaf.lock().unwrap().circuits.push(circuit.clone());
    }

    if sub_circuit.is_some() {
        sub_circuit.as_ref().unwrap().lock().unwrap().parent = Some(circuit.clone());
    }
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
                        write!(f, "{}", model_circuit.lock().unwrap())?
                    } else {
                        write!(f, " * {}", model_circuit.lock().unwrap())?
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
