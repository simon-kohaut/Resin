// Standard library
use std::ops;
use std::sync::{Arc, Mutex};

// Resin
use crate::circuit::SharedLeaf;
use crate::circuit::SharedReactiveCircuit;

use super::leaf;
use super::ReactiveCircuit;

pub type SharedModel = Arc<Mutex<Model>>;

pub struct Model {
    pub leafs: Vec<SharedLeaf>,
    pub circuit: Option<SharedReactiveCircuit>,
    pub parent: Option<SharedReactiveCircuit>,
}

impl Model {
    pub fn new(
        leafs: &[SharedLeaf],
        circuit: &Option<SharedReactiveCircuit>,
        parent: &Option<SharedReactiveCircuit>,
    ) -> Self {
        let mut model = Model::empty_new(&None);

        for leaf in leafs {
            model.append(leaf);
        }

        model.circuit = circuit.clone();
        if parent.is_some() {
            model.set_parent(&parent.as_ref().unwrap());
            parent.as_ref().unwrap().lock().unwrap().add_model(&model);
        }

        model
    }

    pub fn empty_new(parent: &Option<SharedReactiveCircuit>) -> Self {
        Self {
            leafs: vec![],
            circuit: None,
            parent: parent.clone(),
        }
    }

    pub fn share(&self) -> SharedModel {
        Arc::new(Mutex::new(self.copy()))
    }

    // Read interface
    pub fn get_value(&self) -> (f64, usize) {
        let mut operations_count = if self.leafs.len() < 2 {
            0
        } else {
            self.leafs.len() - 1
        };
        let mut product = self
            .leafs
            .iter()
            .fold(1.0, |acc, leaf| leaf.lock().unwrap().get_value() * acc);

        match &self.circuit {
            Some(circuit) => {
                let (circuit_value, circuit_operations) = circuit.lock().unwrap().get_value();
                product *= circuit_value;
                operations_count += circuit_operations;

                if self.leafs.len() > 1 {
                    operations_count += 1;
                }
            }
            None => (),
        }

        (product, operations_count)
    }

    pub fn contains(&self, leaf: &SharedLeaf) -> bool {
        for own_leaf in self.leafs.iter() {
            if Arc::ptr_eq(&own_leaf, &leaf) {
                return true;
            }
        }

        false
    }

    pub fn is_empty(&self) -> bool {
        self.leafs.is_empty() && self.circuit.is_none()
    }

    pub fn is_leaf(&self) -> bool {
        self.leafs.len() == 1 && self.circuit.is_none()
    }

    pub fn is_circuit(&self) -> bool {
        self.leafs.is_empty() && self.circuit.is_some()
    }

    pub fn copy(&self) -> Model {
        let mut copy = Model::new(&vec![], &None, &self.parent);

        for leaf in &self.leafs {
            copy.append(&leaf);
        }

        match &self.circuit {
            Some(circuit) => copy.circuit = Some(circuit.clone()),
            None => (),
        }

        copy
    }

    // Write interface
    pub fn set_parent(&mut self, parent: &SharedReactiveCircuit) {
        // Set sub-circuit parameters
        if self.circuit.is_some() {
            self.circuit.as_ref().unwrap().lock().unwrap().parent = Some(parent.clone());
        }

        // If there was a prior parent, remove reference from leafs
        if self.parent.is_some() {
            for leaf in &self.leafs {
                leaf.lock()
                    .unwrap()
                    .remove_circuit(&self.parent.as_ref().unwrap());
            }
        }

        // Set new parent for all leafs
        for leaf in &self.leafs {
            leaf.lock().unwrap().circuits.push(parent.clone());
        }
    }

    pub fn append(&mut self, leaf: &SharedLeaf) {
        self.leafs.push(leaf.clone());

        if self.parent.is_some() {
            leaf.lock()
                .unwrap()
                .circuits
                .push(self.parent.as_ref().unwrap().clone());
        }
    }

    pub fn remove(&mut self, leaf: &SharedLeaf) {
        self.leafs.retain(|l| !Arc::ptr_eq(&l, &leaf));

        if self.parent.is_some() {
            leaf.lock()
                .unwrap()
                .remove_circuit(&self.parent.as_ref().unwrap().clone());
        }
    }

    pub fn empty(&mut self) {
        self.leafs = Vec::new();
        self.circuit = None;
    }

    pub fn new_circuit(&mut self) {
        self.circuit = Some(ReactiveCircuit::empty_new().share());

        if self.parent.is_some() {
            self.circuit.as_ref().unwrap().lock().unwrap().parent = self.parent.clone();
        }
    }

    pub fn disconnect(&mut self) {
        if self.parent.is_some() {
            self.circuit.as_ref().unwrap().lock().unwrap().parent = None;

            for leaf in &self.leafs {
                leaf.lock()
                    .unwrap()
                    .remove_circuit(&self.parent.as_ref().unwrap());
            }

            self.parent = None;
        }
    }
}

impl ops::Mul<SharedLeaf> for Model {
    type Output = Model;

    fn mul(self, _rhs: SharedLeaf) -> Model {
        let mut multiplied = self.copy();
        multiplied.append(&_rhs);
        multiplied
    }
}

impl ops::Div<SharedLeaf> for Model {
    type Output = Model;

    fn div(self, _rhs: SharedLeaf) -> Model {
        let mut divided = self.copy();
        divided.remove(&_rhs);
        divided
    }
}
