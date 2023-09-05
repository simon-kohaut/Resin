use std::sync::{Arc, Mutex};
use std::vec::Vec;

#[derive(Debug)]
pub struct Leaf {
    value: f64,
    frequency: f64,
    pub name: String,
}

pub type SharedLeaf = Arc<Mutex<Leaf>>;

pub fn shared_leaf(value: f64, frequency: f64, name: String) -> SharedLeaf {
    Arc::new(Mutex::new(Leaf {
        value,
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
    }
}
