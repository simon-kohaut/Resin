use crate::circuit::{add_model, Model, ReactiveCircuit, SharedLeaf, SharedReactiveCircuit};

pub fn lift_leaf(circuit: &SharedReactiveCircuit, leaf: &SharedLeaf) {
    let mut circuit_guard = circuit.lock().unwrap();
    let circuit_layer = circuit_guard.layer;

    // Assume we will only visit a circuit containing this leaf if
    // it is the root circuit. Otherwise, we remove the leaf beforehand to
    // not require a reference to the parent circuit
    if circuit_guard.contains(&leaf) {
        // A new root with two models, one containing the relevant leaf and a circuit
        // of all models with that leaf, one with all models that do not contain the leaf
        let mut root_circuit = ReactiveCircuit::empty_new();
        let non_leaf_circuit = ReactiveCircuit::empty_new().share();
        let leaf_circuit = ReactiveCircuit::empty_new().share();

        for model in &mut circuit_guard.models {
            // Let leafs of this model forget about this circuit
            for model_leaf in &mut model.leafs {
                model_leaf.lock().unwrap().remove_circuit(&circuit);
            }

            // Remove the leaf from all models of this circuit
            if model.contains(leaf) {
                model.remove(leaf);
                add_model(&leaf_circuit, &model.leafs, &model.circuit);
            }
            // Push models to their own circuit if they are independent of the leaf
            else {
                add_model(&non_leaf_circuit, &model.leafs, &model.circuit);
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
            .push(Model::new(&Vec::new(), &Some(non_leaf_circuit)));
        root_circuit
            .models
            .push(Model::new(&vec![leaf.clone()], &Some(leaf_circuit)));
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
                    .contains(&leaf)
                {
                    // Build new circuit for parts that are irrelevant to leaf
                    let mut non_leaf_circuit = ReactiveCircuit::empty_new();
                    non_leaf_circuit.layer = circuit_layer + 1;
                    add_model(
                        &model.circuit.as_ref().unwrap(),
                        &model.leafs,
                        &Some(non_leaf_circuit.share()),
                    );

                    // Make this model react to leaf
                    model.append(leaf);

                    // Let leaf reference the new circuit and forget about the original
                    leaf.lock().unwrap().remove_circuit(&circuit);
                    leaf.lock()
                        .unwrap()
                        .circuits
                        .push(model.circuit.as_mut().unwrap().clone());

                    let mut model_circuit_guard = model.circuit.as_mut().unwrap().lock().unwrap();
                    for inner_model in &mut model_circuit_guard.models {
                        if !inner_model.contains(&leaf) {
                            non_leaf_circuit.models.push(inner_model.copy());
                            inner_model.empty();
                        }
                    }
                    model_circuit_guard.remove(&leaf);
                } else {
                    lift_leaf(&model.circuit.as_mut().unwrap(), &leaf);
                }
            }
        }
    }
}

pub fn drop_leaf(circuit: &SharedReactiveCircuit, leaf: &SharedLeaf) {
    let mut circuit_guard = circuit.lock().unwrap();
    let circuit_layer = circuit_guard.layer + 1;
    if circuit_guard.contains(&leaf) {
        // Remove this circuit from being referenced by the leaf
        leaf.lock().unwrap().remove_circuit(&circuit);

        for model in &mut circuit_guard.models {
            if model.contains(&leaf) {
                model.remove(leaf);

                match &mut model.circuit {
                    Some(model_circuit) => {
                        let mut model_circuit_guard = model_circuit.lock().unwrap();
                        for circuit_model in &mut model_circuit_guard.models {
                            circuit_model.append(leaf);
                        }
                    }
                    None => {
                        model.circuit = Some(ReactiveCircuit::empty_new().share());
                        model.circuit.as_ref().unwrap().lock().unwrap().parent =
                            Some(circuit.clone());
                        model.circuit.as_ref().unwrap().lock().unwrap().layer = circuit_layer;
                        add_model(&model.circuit.as_ref().unwrap(), &vec![leaf.clone()], &None);
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
                Some(model_circuit) => drop_leaf(&model_circuit, leaf),
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
