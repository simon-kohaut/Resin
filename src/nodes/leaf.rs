use std::sync::{Arc, Mutex};

use crate::nodes::SharedOperator;

pub struct Leaf {
    value: f64,
    pub parents: Vec<SharedOperator>,
    pub frequency: f64,
    pub name: String,
}

pub type SharedLeaf = Arc<Mutex<Leaf>>;

pub fn shared_leaf(value: f64, frequency: f64, name: String) -> SharedLeaf {
    Arc::new(Mutex::new(Leaf {
        value,
        parents: Vec::new(),
        frequency,
        name,
    }))
}

impl Leaf {
    pub fn get_value(&self) -> f64 {
        self.value
    }

    pub fn set_value(&mut self, value: f64) {
        self.value = value;
        for parent in &self.parents {
            parent.lock().unwrap().invalidate();
        }
    }
}
