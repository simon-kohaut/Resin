use std::sync::{Arc, Mutex};

use super::SharedReactiveCircuit;

#[derive(Debug)]
pub struct Leaf {
    value: f64,
    frequency: f64,
    pub name: String,
    pub circuits: Vec<SharedReactiveCircuit>,
}

pub type SharedLeaf = Arc<Mutex<Leaf>>;

pub fn shared_leaf(value: f64, frequency: f64, name: String) -> SharedLeaf {
    Arc::new(Mutex::new(Leaf {
        value,
        frequency,
        name,
        circuits: vec![],
    }))
}

impl Leaf {
    pub fn get_value(&self) -> f64 {
        self.value
    }

    pub fn set_value(&mut self, value: f64) {
        self.value = value;
    }

    pub fn remove_circuit(&mut self, circuit: SharedReactiveCircuit) {
        self.circuits.retain(|c| !Arc::ptr_eq(c, &circuit))
    }
}

pub type SharedResult = Arc<Mutex<f64>>;
