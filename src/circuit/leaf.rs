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

pub fn shared_leaf(value: f64, frequency: f64, name: &str) -> SharedLeaf {
    Arc::new(Mutex::new(Leaf {
        value,
        frequency,
        name: name.to_owned(),
        circuits: vec![],
    }))
}

impl Leaf {
    pub fn new(value: &f64, frequency: &f64, name: &str) -> Self {
        Self {
            value: *value,
            frequency: *frequency,
            name: name.to_owned(),
            circuits: vec![],
        }
    }

    pub fn share(&self) -> SharedLeaf {
        Arc::new(Mutex::new(self.copy()))
    }

    pub fn copy(&self) -> Leaf {
        let mut copy = Leaf::new(&self.value, &self.frequency, &self.name);
        for circuit in &self.circuits {
            copy.circuits.push(circuit.clone());
        }
        copy
    }

    pub fn get_value(&self) -> f64 {
        self.value
    }

    pub fn set_value(&mut self, value: &f64) {
        self.value = *value;
    }

    pub fn remove_circuit(&mut self, circuit: &SharedReactiveCircuit) {
        self.circuits.retain(|c| !Arc::ptr_eq(c, &circuit))
    }
}

pub fn update(leaf: &SharedLeaf, value: &f64) {
    leaf.lock().unwrap().set_value(value);
    let circuits = leaf.lock().unwrap().circuits.clone();
    for circuit in circuits {
        circuit.lock().unwrap().invalidate();
    }
}
