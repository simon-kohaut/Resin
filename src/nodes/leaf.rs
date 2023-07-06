use crate::nodes::SharedOperator;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::vec::Vec;

#[derive(Debug)]
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

impl fmt::Display for Leaf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "(Leaf {} = {})", self.name, self.value)
    }
}
