use crate::circuit::{Model, ReactiveCircuit, SharedLeaf, SharedReactiveCircuit};

use super::reactive_circuit::move_model;

pub fn lift_leaf(
    circuit: &SharedReactiveCircuit,
    leaf: &SharedLeaf,
) -> (Option<Model>, Option<Model>, Option<Model>) {
    // Assume we will only visit a circuit containing this leaf if
    // it is the root circuit. Otherwise, we remove the leaf beforehand to
    // not require a reference to the parent circuit
    if circuit.lock().unwrap().contains(&leaf) {
        // A new root with potentially three models
        // - The leaf on its own
        // - The leaf with a circuit
        // - The sub-tree that is independent of the leaf
        let mut root_model = None;
        let mut leaf_circuit = None;
        let mut non_leaf_circuit = None;

        // Move the models to the appropriate new place
        for model in &mut circuit.lock().unwrap().models {
            if model.contains(leaf) {
                if model.is_leaf() {
                    root_model = Some(model.copy());
                } else {
                    model.remove(leaf);
                    if leaf_circuit.is_none() {
                        leaf_circuit = Some(ReactiveCircuit::empty_new().share())
                    }
                    move_model(&leaf_circuit.as_ref().unwrap(), model);
                }
            } else {
                if non_leaf_circuit.is_none() {
                    non_leaf_circuit = Some(ReactiveCircuit::empty_new().share())
                }
                move_model(&non_leaf_circuit.as_ref().unwrap(), model);
            }
        }

        let leaf_model = if leaf_circuit.is_some() {
            Some(Model::new(&vec![leaf.clone()], &leaf_circuit, &None))
        } else {
            None
        };
        let non_leaf_model = if non_leaf_circuit.is_some() {
            Some(Model::new(&vec![], &non_leaf_circuit, &None))
        } else {
            None
        };

        if circuit.lock().unwrap().parent.is_some() {
            return (root_model, leaf_model, non_leaf_model);
        } else {
            // Override the input circuit to mirror our changes
            circuit.lock().unwrap().empty();
            if root_model.is_some() {
                move_model(circuit, &mut root_model.unwrap());
            }
            if leaf_model.is_some() {
                move_model(circuit, &mut leaf_model.unwrap());
            }
            if non_leaf_model.is_some() {
                move_model(circuit, &mut non_leaf_model.unwrap());
            }

            (None, None, None)
        }
    } else {
        let mut lifted_models = vec![];
        for model in &mut circuit.lock().unwrap().models {
            match &model.circuit {
                Some(model_circuit) => {
                    let (mut root_model, mut leaf_model, mut non_leaf_model) =
                        lift_leaf(&model_circuit, leaf);
                    if root_model.is_none() && leaf_model.is_none() && non_leaf_model.is_none() {
                        continue;
                    }

                    if root_model.is_some() {
                        for leaf in &model.leafs {
                            root_model.as_mut().unwrap().append(&leaf);
                        }
                        lifted_models.push(root_model.unwrap());
                    }
                    if leaf_model.is_some() {
                        for leaf in &model.leafs {
                            leaf_model.as_mut().unwrap().append(&leaf);
                        }
                        lifted_models.push(leaf_model.unwrap());
                    }
                    if non_leaf_model.is_some() {
                        for leaf in &model.leafs {
                            non_leaf_model.as_mut().unwrap().append(&leaf);
                        }
                        lifted_models.push(non_leaf_model.unwrap());
                    }

                    model.empty();
                }
                None => (),
            }
        }

        for lifted_model in &mut lifted_models {
            move_model(circuit, lifted_model);
        }

        (None, None, None)
    }
}

pub fn drop_leaf(circuit: &SharedReactiveCircuit, leaf: &SharedLeaf) {
    if circuit.lock().unwrap().contains(&leaf) {
        // Remove this circuit from being referenced by the leaf
        leaf.lock().unwrap().remove_circuit(&circuit);

        for model in &mut circuit.lock().unwrap().models {
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
                        let _ = Model::new(&vec![leaf.clone()], &None, &model.circuit);
                    }
                }
            }
        }
    } else {
        for model in &mut circuit.lock().unwrap().models {
            match &model.circuit {
                Some(model_circuit) => drop_leaf(&model_circuit, leaf),
                None => (),
            }
        }
    }
}

pub fn prune(circuit: Option<SharedReactiveCircuit>) -> Option<SharedReactiveCircuit> {
    match circuit {
        Some(circuit) => {
            // Prune underlying circuits
            for model in &mut circuit.lock().unwrap().models {
                model.circuit = prune(model.circuit.clone());
            }

            // Remove empty models
            circuit
                .lock()
                .unwrap()
                .models
                .retain(|m| m.leafs.len() > 0 || m.circuit.is_some());

            // Simplify sum over RCs without leafs
            if circuit.lock().unwrap().layer == 0 {
                let no_leafs = circuit
                    .lock()
                    .unwrap()
                    .models
                    .iter()
                    .fold(true, |acc, model| acc && model.leafs.len() == 0);
                if no_leafs {
                    let root = ReactiveCircuit::empty_new().share();
                    for model in &circuit.lock().unwrap().models {
                        for inner_model in
                            &mut model.circuit.as_ref().unwrap().lock().unwrap().models
                        {
                            move_model(&root, inner_model);
                        }
                    }

                    circuit.lock().unwrap().empty();
                    for model in &mut root.lock().unwrap().models {
                        move_model(&circuit, model)
                    }
                }
            }

            // Remove this circuit if it is empty
            if circuit.lock().unwrap().models.is_empty() {
                return None;
            } else {
                return Some(circuit.clone());
            }
        }
        None => {
            return None;
        }
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
