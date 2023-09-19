// Standard library
use std::{
    fs::File,
    io::prelude::*,
    process::Command,
    str::FromStr,
    sync::{Arc, Mutex},
};

// Resin
use crate::circuit::Model;
use crate::circuit::SharedLeaf;

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
        dot_text += &self.to_dot_file(&mut 0);
        dot_text += "}";

        let dot_path = path.to_owned() + ".dot";
        let mut f = File::create(&dot_path)?;
        f.write_all(dot_text.as_bytes())?;
        f.sync_all()?;

        let svg_text = Command::new("dot")
            .args(["-Tsvg", &dot_path])
            .output()
            .expect("Failed to run graphviz!");

        let svg_path = path.to_owned() + ".svg";
        let mut f = File::create(svg_path)?;
        f.write_all(&svg_text.stdout)?;
        f.sync_all()?;

        Ok(())
    }

    pub fn to_dot_file(&self, index: &mut i32) -> String {
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
                dot_text += &String::from_str(&format!(
                    "\"{name}\n{value:.2}\" [{color}]\n",
                    name = name,
                    color = color,
                    value = value,
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

                    dot_text += &model_circuit.lock().unwrap().to_dot_file(index);
                }
                None => (),
            }

            model_index += 1;
        }

        dot_text
    }

    // Write interface
    pub fn get_value(&mut self) -> f64 {
        if !self.valid {
            self.update()
        }

        self.value
    }

    pub fn update(&mut self) {
        let mut sum = 0.0;

        for model in &self.models {
            sum += model.value();
        }

        self.value = sum;
        self.valid = true;
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
