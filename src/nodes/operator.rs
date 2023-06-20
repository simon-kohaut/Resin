use ndarray::{Array, Array1, Axis, array, concatenate};
use std::sync::{Arc, Mutex};

use crate::nodes::SharedLeaf;

pub struct Operator {
    pub value: f64,
    leafs: Vec<SharedLeaf>,
    operators: Vec<SharedOperator>,
    weights: Array1<f64>,
    operation: Operation,
    valid: bool,
    parents: Vec<SharedOperator>,
}

pub type SharedOperator = Arc<Mutex<Operator>>;

enum Operation {
    Sum,
    Product,
    Max,
}

impl Operator {
    pub fn update(&mut self) {
        // If this node is valid, no need to do anything
        if self.valid {
            return
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
                self.value = Array::from_vec(operands).dot(&self.weights);
            }
            Operation::Product => {
                self.value = Array::from_vec(operands).product();
            }
            Operation::Max => {
                // TODO: Set max rather than sum
                self.value = Array::from_vec(operands).dot(&self.weights);
            }
        }

        // After recomputing the value, this node is valid again
        self.valid = true;
    }

    pub fn contains(&self, name: &String) -> bool {
        for leaf in self.leafs.iter() {
            if leaf.lock().unwrap().name == *name {
                return true;
            }
        }

        for operator in self.operators.iter() {
            if operator.lock().unwrap().contains(&name) {
                return true;
            }
        }

        false
    }

    pub fn invalidate(&mut self) {
        self.valid = false;
        for parent in &self.parents {
            parent.lock().unwrap().invalidate();
        }
    }
}

pub fn add_leaf(leaf: SharedLeaf, operator: SharedOperator) {
    leaf.lock().unwrap().parents.push(operator.clone());
    let mut operator_access = operator.lock().unwrap();
    operator_access.leafs.push(leaf.clone());
    operator_access.invalidate();
    operator_access.weights = concatenate(Axis(0), &[operator_access.weights.view(), array![1.0].view()]).unwrap();
}

pub fn add_operator(operator: SharedOperator, parent: SharedOperator) {
    operator.lock().unwrap().parents.push(parent.clone());
    let mut parent_access = parent.lock().unwrap();
    parent_access.operators.push(operator.clone());
    parent_access.invalidate();
    parent_access.weights = concatenate(Axis(0), &[parent_access.weights.view(), array![1.0].view()]).unwrap();
}

pub fn sum_node() -> SharedOperator {
    Arc::new(Mutex::new(Operator {
        value: 0.0,
        leafs: Vec::new(),
        operators: Vec::new(),
        weights: Array1::from_vec(Vec::new()),
        operation: Operation::Sum,
        valid: false,
        parents: Vec::new(),
    }))
}

pub fn product_node() -> SharedOperator {
    Arc::new(Mutex::new(Operator {
        value: 0.0,
        leafs: Vec::new(),
        operators: Vec::new(),
        weights: Array1::from_vec(Vec::new()),
        operation: Operation::Product,
        valid: false,
        parents: Vec::new(),
    }))
}

pub fn max_node() -> SharedOperator {
    Arc::new(Mutex::new(Operator {
        value: 0.0,
        leafs: Vec::new(),
        operators: Vec::new(),
        weights: Array1::from_vec(Vec::new()),
        operation: Operation::Max,
        valid: false,
        parents: Vec::new(),
    }))
}
