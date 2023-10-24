use crate::circuit::{self, Model, ReactiveCircuit, SharedLeaf, SharedReactiveCircuit};

use super::{leaf::move_leafs, reactive_circuit::move_model};

use itertools::Itertools;
use rayon::prelude::*;

pub fn search_leaf(
    circuit: &SharedReactiveCircuit,
    leaf: &SharedLeaf,
) -> Vec<SharedReactiveCircuit> {
    if circuit.lock().unwrap().contains(leaf) {
        return vec![circuit.clone()];
    } else {
        let mut circuits: Vec<SharedReactiveCircuit> = vec![];
        for model in &circuit.lock().unwrap().models {
            if model.circuit.is_some() {
                circuits.append(&mut search_leaf(&model.circuit.as_ref().unwrap(), leaf));
            }
        }
        return circuits;
    }
}

pub fn search_leaf_ahead(
    circuit: &SharedReactiveCircuit,
    leaf: &SharedLeaf,
) -> Vec<SharedReactiveCircuit> {
    // Check if the next layer of circuits has the searched leaf
    let mut circuits: Vec<SharedReactiveCircuit> = vec![];
    for model in &circuit.lock().unwrap().models {
        if model.circuit.is_some() {
            if model
                .circuit
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .contains(leaf)
            {
                circuits.push(circuit.clone());
                break;
            }
        }
    }

    // Either the next layer has the circuit and we can go
    // or this is still empty and the search goes on in the next layer
    if circuits.is_empty() {
        for model in &circuit.lock().unwrap().models {
            if model.circuit.is_some() {
                circuits.append(&mut search_leaf_ahead(
                    &model.circuit.as_ref().unwrap(),
                    leaf,
                ));
            }
        }
    }

    return circuits;
}

pub fn split_by_leaf(circuit: &SharedReactiveCircuit, leaf: &SharedLeaf) -> (Model, Model, Model) {
    let mut singleton = Model::empty_new(&None); // Model containing only the leaf
    let mut in_scope = Model::empty_new(&None); // Model of equation multiplied by leaf
    let mut out_of_scope = Model::empty_new(&None); // Model of equation independent of leaf

    println!("Split by leaf");

    // Fill optional models accordingly
    for model in &mut circuit.lock().unwrap().models {
        if model.contains(leaf) {
            if model.is_leaf() {
                singleton.append(leaf);
            } else {
                if in_scope.circuit.is_none() {
                    in_scope.new_circuit();
                    in_scope.append(leaf);
                }

                model.remove(leaf);
                move_model(&in_scope.circuit.as_ref().unwrap(), model);
            }
        } else {
            if out_of_scope.circuit.is_none() {
                out_of_scope.new_circuit();
            }

            move_model(&out_of_scope.circuit.as_ref().unwrap(), model);
        }

        model.empty();
    }

    println!("Split end");

    (singleton, in_scope, out_of_scope)
}

pub fn parallel_lift_leaf(circuits: Vec<SharedReactiveCircuit>, leaf: &SharedLeaf) {
    circuits
        .par_iter()
        .for_each(|circuit| lift_leaf(&circuit, leaf));
}

pub fn parallel_drop_leaf(circuits: Vec<SharedReactiveCircuit>, leaf: &SharedLeaf) {
    circuits
        .par_iter()
        .for_each(|circuit| drop_leaf(&circuit, leaf));
}

pub fn lift_leaf(circuit: &SharedReactiveCircuit, leaf: &SharedLeaf) {
    let mut lifted_models = vec![];
    for model in &mut circuit.lock().unwrap().models {
        match model.circuit.as_ref() {
            Some(model_circuit) => {
                if model_circuit.lock().unwrap().contains(leaf) {
                    let (mut singleton, mut in_scope, mut out_of_scope) =
                        split_by_leaf(&model_circuit, leaf);
                    if !singleton.is_empty() {
                        move_leafs(&mut singleton, model);
                        lifted_models.push(singleton);
                    }
                    if !in_scope.is_empty() {
                        move_leafs(&mut in_scope, model);
                        lifted_models.push(in_scope);
                    }
                    if !out_of_scope.is_empty() {
                        move_leafs(&mut out_of_scope, model);
                        lifted_models.push(out_of_scope);
                    }
                    model.empty();
                    println!("Worked through a model");
                }
            }
            None => (),
        }
    }

    println!("Move models");

    for model in &mut lifted_models {
        move_model(&circuit, model);
    }

    println!("Moved models");
}

pub fn drop_leaf(circuit: &SharedReactiveCircuit, leaf: &SharedLeaf) {
    let mut circuit_guard = circuit.lock().unwrap();
    if circuit_guard.contains(&leaf) {
        // Remove this circuit from being referenced by the leaf
        leaf.lock().unwrap().remove_circuit(&circuit);

        for model in &mut circuit_guard.models {
            if model.contains(&leaf) {
                model.remove(leaf);

                match &mut model.circuit {
                    Some(model_circuit) => {
                        for circuit_model in &mut model_circuit.lock().unwrap().models {
                            circuit_model.append(leaf);
                        }
                    }
                    None => {
                        model.new_circuit();
                        Model::new(&vec![leaf.clone()], &None, &model.circuit);
                    }
                }
            }
        }
    } else {
        for model in &mut circuit_guard.models {
            match &model.circuit {
                Some(model_circuit) => drop_leaf(&model_circuit, leaf),
                None => (),
            }
        }
    }
}

pub fn merge(circuits: Vec<SharedReactiveCircuit>) -> SharedReactiveCircuit {
    let merged = ReactiveCircuit::empty_new().share();

    let mut models = vec![];
    for circuit in circuits {
        for model in &mut circuit.lock().unwrap().models {
            models.push(model.copy());
            model.empty();
        }

        circuit.lock().unwrap().empty();
    }

    for model in &mut models {
        move_model(&merged, model);
    }

    merged
}

pub fn prune(optional_circuit: Option<SharedReactiveCircuit>) -> Option<SharedReactiveCircuit> {
    if optional_circuit.is_none() {
        return None;
    }

    let circuit = optional_circuit.unwrap();

    // Simplify sum over RCs without leafs
    println!("Merge candidates");
    let merge_candidates: Vec<SharedReactiveCircuit> = circuit
        .lock()
        .unwrap()
        .models
        .iter()
        .filter(|model| model.is_circuit())
        .map(|model| model.circuit.as_ref().unwrap())
        .cloned()
        .collect();
    if merge_candidates.len() > 1 {
        println!("Merging");
        let merged = merge(merge_candidates.iter().cloned().collect());
        for candidate in &merge_candidates {
            candidate.lock().unwrap().empty();
        }
        Model::new(&vec![], &Some(merged), &Some(circuit.clone()));
    }

    // Prune underlying circuits
    println!("Prune subcircuits");
    for model in &mut circuit.lock().unwrap().models {
        model.circuit = prune(model.circuit.clone());
    }

    // Remove empty models
    println!("Remove empties");
    circuit.lock().unwrap().models.retain(|m| !m.is_empty());

    // Remove this circuit if it is empty
    if circuit.lock().unwrap().models.is_empty() {
        return None;
    } else {
        return Some(circuit.clone());
    }
}
