use ndarray::Array;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use crate::nodes::SharedLeaf;

#[derive(Debug)]
pub struct Operator {
    pub value: f64,
    leafs: Vec<SharedLeaf>,
    operators: Vec<SharedOperator>,
    operation: Operation,
    valid: bool,
    parents: Vec<SharedOperator>,
    domain: HashSet<String>,
}

pub type SharedOperator = Arc<Mutex<Operator>>;

#[derive(Debug)]
enum Operation {
    Sum,
    Product,
    Max,
}

impl Operator {
    pub fn update(&mut self) {
        // If this node is valid, no need to do anything
        if self.valid {
            return;
        }

        // Let all child operator nodes update their values first
        for operator in self.operators.iter() {
            operator.lock().unwrap().update();
        }

        // Gather updated values of operators and leaf nodes
        let mut operator_values: Vec<f64> = self
            .operators
            .iter()
            .map(|operator| operator.lock().unwrap().value)
            .collect();
        let mut leaf_values: Vec<f64> = self
            .leafs
            .iter()
            .map(|leaf| leaf.lock().unwrap().get_value())
            .collect();

        // Concatenate values to get all operands in ndarray
        let mut operands: Vec<f64> = Vec::new();
        operands.append(&mut operator_values);
        operands.append(&mut leaf_values);

        // Recompute own value
        match self.operation {
            Operation::Sum => {
                self.value = Array::from_vec(operands).sum();
            }
            Operation::Product => {
                self.value = Array::from_vec(operands).product();
            }
            Operation::Max => {
                // TODO: Set max rather than sum
                self.value = Array::from_vec(operands).sum();
            }
        }

        // After recomputing the value, this node is valid again
        self.valid = true;
    }

    pub fn invalidate(&mut self) {
        self.valid = false;
        for parent in &self.parents {
            let guard = parent.try_lock();
            match guard {
                Ok(_) => guard.unwrap().invalidate(),
                Err(_) => return, // This is the locked root node
            }
        }
    }

    pub fn leafs_contain(&self, searched_leaf: &SharedLeaf) -> bool {
        for leaf in self.leafs.iter() {
            if Arc::ptr_eq(&leaf, &searched_leaf) {
                return true;
            }
        }

        false
    }

    pub fn operators_contain(&self, searched_leaf: &SharedLeaf) -> bool {
        for operator in &self.operators {
            if operator.lock().unwrap().contains(&searched_leaf) {
                return true;
            }
        }

        false
    }

    pub fn contains(&self, searched_leaf: &SharedLeaf) -> bool {
        self.leafs_contain(searched_leaf) || self.operators_contain(searched_leaf)
    }

    pub fn remove_from_leafs(&mut self, leaf: &SharedLeaf) {
        let number_leafs_before = self.leafs.len();

        self.leafs.retain(|l| Arc::ptr_eq(&l, &leaf));

        if self.leafs.len() < number_leafs_before {
            self.invalidate();
        }
    }

    pub fn remove_from_operators(&mut self, leaf: &SharedLeaf) {
        for operator in &self.operators {
            operator.lock().unwrap().remove(&leaf);
        }
    }

    pub fn remove(&mut self, leaf: &SharedLeaf) {
        self.remove_from_operators(leaf);
        self.remove_from_leafs(leaf);
    }

    pub fn prune(&mut self) {
        for operator in &self.operators {
            operator.lock().unwrap().prune();
        }

        self.operators
            .retain(|o| o.lock().unwrap().leafs.len() > 0 || o.lock().unwrap().operators.len() > 0)
    }

    pub fn update_domain(&mut self) {
        for operator in &self.operators {
            self.domain.extend(operator.lock().unwrap().domain.clone());
        }

        for parent in &self.parents {
            parent.lock().unwrap().update_domain();
        }
    }
}

pub fn add_leaf(leaf: SharedLeaf, operator: SharedOperator) {
    leaf.lock().unwrap().parents.push(operator.clone());
    let mut operator_access = operator.lock().unwrap();
    operator_access.leafs.push(leaf.clone());
    operator_access.invalidate();
    operator_access
        .domain
        .insert(leaf.lock().unwrap().name.clone());
}

pub fn add_operator(operator: SharedOperator, parent: SharedOperator) {
    operator.lock().unwrap().parents.push(parent.clone());
    let mut parent_access = parent.lock().unwrap();
    parent_access.operators.push(operator.clone());
    parent_access.invalidate();
    parent_access.update_domain();
}

pub fn sum_node() -> SharedOperator {
    Arc::new(Mutex::new(Operator {
        value: 0.0,
        leafs: Vec::new(),
        operators: Vec::new(),
        operation: Operation::Sum,
        valid: false,
        parents: Vec::new(),
        domain: HashSet::new(),
    }))
}

pub fn product_node() -> SharedOperator {
    Arc::new(Mutex::new(Operator {
        value: 0.0,
        leafs: Vec::new(),
        operators: Vec::new(),
        operation: Operation::Product,
        valid: false,
        parents: Vec::new(),
        domain: HashSet::new(),
    }))
}

pub fn max_node() -> SharedOperator {
    Arc::new(Mutex::new(Operator {
        value: 0.0,
        leafs: Vec::new(),
        operators: Vec::new(),
        operation: Operation::Max,
        valid: false,
        parents: Vec::new(),
        domain: HashSet::new(),
    }))
}
