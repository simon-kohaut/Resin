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

#[derive(Debug)]
pub struct ReactiveCircuit {
    pub models: Vec<Model>,
    parent: Option<SharedReactiveCircuit>,
    layer: i32,
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
    pub fn contains(&self, leaf: SharedLeaf) -> bool {
        for model in &self.models {
            if model.contains(leaf.clone()) {
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

    pub fn remove(&mut self, leaf: SharedLeaf) {
        for model in &mut self.models {
            model.remove(leaf.clone());
        }
    }
}

pub fn update(leaf: SharedLeaf, value: f64) {
    leaf.lock().unwrap().set_value(value);
    let circuits = leaf.lock().unwrap().circuits.clone();
    for circuit in circuits {
        circuit.lock().unwrap().invalidate();
    }
}

pub fn add_model(
    circuit: SharedReactiveCircuit,
    leafs: Vec<SharedLeaf>,
    sub_circuit: Option<SharedReactiveCircuit>,
) {
    let model = Model::new(leafs.clone(), sub_circuit.clone());
    circuit.lock().unwrap().models.push(model);

    for leaf in &leafs {
        leaf.lock().unwrap().circuits.push(circuit.clone());
    }

    if sub_circuit.is_some() {
        sub_circuit.as_ref().unwrap().lock().unwrap().parent = Some(circuit.clone());
    }
}

pub fn lift_leaf(circuit: SharedReactiveCircuit, leaf: SharedLeaf) {
    let mut circuit_guard = circuit.lock().unwrap();
    let circuit_layer = circuit_guard.layer;

    // Assume we will only visit a circuit containing this leaf if
    // it is the root circuit. Otherwise, we remove the leaf beforehand to
    // not require a reference to the parent circuit
    if circuit_guard.contains(leaf.clone()) {
        // A new root with two models, one containing the relevant leaf and a circuit
        // of all models with that leaf, one with all models that do not contain the leaf
        let mut root_circuit = ReactiveCircuit::empty_new();
        let non_leaf_circuit = ReactiveCircuit::empty_new().share();
        let leaf_circuit = ReactiveCircuit::empty_new().share();

        for model in &mut circuit_guard.models {
            // Let leafs of this model forget about this circuit
            for model_leaf in &mut model.leafs {
                model_leaf.lock().unwrap().remove_circuit(circuit.clone());
            }

            // Remove the leaf from all models of this circuit
            if model.contains(leaf.clone()) {
                model.remove(leaf.clone());
                add_model(
                    leaf_circuit.clone(),
                    model.leafs.clone(),
                    model.circuit.clone(),
                );
            }
            // Push models to their own circuit if they are independent of the leaf
            else {
                add_model(
                    non_leaf_circuit.clone(),
                    model.leafs.clone(),
                    model.circuit.clone(),
                );
            }
        }

        // Set the correct layer numbering
        root_circuit.layer = circuit_guard.layer;
        leaf_circuit.lock().unwrap().layer = circuit_guard.layer + 1;
        leaf_circuit.lock().unwrap().parent = Some(circuit.clone());
        non_leaf_circuit.lock().unwrap().layer = circuit_guard.layer + 1;
        non_leaf_circuit.lock().unwrap().parent = Some(circuit.clone());

        // Construct the new root circuits models
        root_circuit
            .models
            .push(Model::new(Vec::new(), Some(non_leaf_circuit)));
        root_circuit
            .models
            .push(Model::new(vec![leaf.clone()], Some(leaf_circuit)));
        *circuit_guard = root_circuit;
    } else {
        for model in &mut circuit_guard.models {
            if model.circuit.is_some() {
                if model
                    .circuit
                    .as_ref()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .contains(leaf.clone())
                {
                    // Build new circuit for parts that are irrelevant to leaf
                    let mut non_leaf_circuit = ReactiveCircuit::empty_new();
                    non_leaf_circuit.layer = circuit_layer + 1;
                    add_model(
                        model.circuit.as_ref().unwrap().clone(),
                        model.leafs.clone(),
                        Some(non_leaf_circuit.share()),
                    );

                    // Make this model react to leaf
                    model.append(leaf.clone());

                    // Let leaf reference the new circuit and forget about the original
                    leaf.lock().unwrap().remove_circuit(circuit.clone());
                    leaf.lock()
                        .unwrap()
                        .circuits
                        .push(model.circuit.as_mut().unwrap().clone());

                    let mut model_circuit_guard = model.circuit.as_mut().unwrap().lock().unwrap();
                    for inner_model in &mut model_circuit_guard.models {
                        if !inner_model.contains(leaf.clone()) {
                            non_leaf_circuit.models.push(inner_model.copy());
                            inner_model.empty();
                        }
                    }
                    model_circuit_guard.remove(leaf.clone());
                } else {
                    lift_leaf(model.circuit.as_mut().unwrap().clone(), leaf.clone());
                }
            }
        }
    }
}

pub fn drop_leaf(circuit: SharedReactiveCircuit, leaf: SharedLeaf) {
    let mut circuit_guard = circuit.lock().unwrap();
    let circuit_layer = circuit_guard.layer + 1;
    if circuit_guard.contains(leaf.clone()) {
        // Remove this circuit from being referenced by the leaf
        leaf.lock().unwrap().remove_circuit(circuit.clone());

        for model in &mut circuit_guard.models {
            if model.contains(leaf.clone()) {
                model.remove(leaf.clone());

                match &mut model.circuit {
                    Some(model_circuit) => {
                        let mut model_circuit_guard = model_circuit.lock().unwrap();
                        for circuit_model in &mut model_circuit_guard.models {
                            circuit_model.append(leaf.clone());
                        }
                    }
                    None => {
                        model.circuit = Some(ReactiveCircuit::empty_new().share());
                        model.circuit.as_ref().unwrap().lock().unwrap().parent =
                            Some(circuit.clone());
                        model.circuit.as_ref().unwrap().lock().unwrap().layer = circuit_layer;
                        add_model(
                            model.circuit.as_ref().unwrap().clone(),
                            vec![leaf.clone()],
                            None,
                        );
                    }
                }

                leaf.lock()
                    .unwrap()
                    .circuits
                    .push(model.circuit.as_ref().unwrap().clone());
            }
        }
    } else {
        for model in &mut circuit_guard.models {
            match &model.circuit {
                Some(model_circuit) => drop_leaf(model_circuit.clone(), leaf.clone()),
                None => (),
            }
        }
    }
}

pub fn prune(circuit: Option<SharedReactiveCircuit>) -> Option<SharedReactiveCircuit> {
    if circuit.is_none() {
        return None;
    }

    let mut circuit_guard = circuit.as_ref().unwrap().lock().unwrap();

    // Prune underlying circuits
    for model in &mut circuit_guard.models {
        model.circuit = prune(model.circuit.clone());
    }

    // Remove empty models
    circuit_guard
        .models
        .retain(|m| m.leafs.len() > 0 || m.circuit.is_some());

    circuit_guard.update();

    // Remove this circuit if it is empty
    if circuit_guard.models.len() == 0 {
        return None;
    } else {
        return circuit.clone();
    }

    // Remove this circuit if its only model is a forwarding of another circuit
    // i.e. unneccessary indirection
    // if circuit_guard.models.len() == 1
    //     && circuit_guard.models[0].leafs.len() == 0
    //     && circuit_guard.layer - circuit_guard.models[0].circuit.as_ref().unwrap().layer > 1
    // {
    //     let lonesome_circuit = circuit_guard.models[0].circuit.unwrap().clone();
    //     circuit_guard = lonesome_circuit.clone();
    // }

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
}

// pub fn fix(circuit: &ReactiveCircuit) -> ReactiveCircuit {
//     let mut fixed_circuit = circuit.copy();

//     for model in &mut fixed_circuit.models {
//         if model.circuit.is_some() {
//             let sub_circuit = model.circuit.as_ref().unwrap();
//             if sub_circuit.layer - circuit.layer > 1 {
//                 let mut buffer = ReactiveCircuit::new();
//                 buffer.layer = circuit.layer + 1;
//                 buffer.add_model(Model::new(Vec::new(), Some(sub_circuit.copy())));
//                 model.circuit = Some(buffer);
//             }
//         }
//     }

//     fixed_circuit
// }

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
