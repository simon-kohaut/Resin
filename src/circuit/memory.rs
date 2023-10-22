use std::sync::{Arc, Mutex};
use chashmap::CHashMap;

use super::add::Add;
use super::mul::Mul;

pub type Memory = Arc<CHashMap<usize, MemoryCell>>;


pub struct MemoryCell {
    pub storage: f64,
    pub valid: bool,
    pub add: Option<Add>,
}


impl MemoryCell {
    // ============================= //
    // ========  CONSTRUCT  ======== //
    pub fn new(storage: f64, valid: bool, add: Option<Add>) -> Self {
        Self {
            storage,
            valid,
            add,
        }
    }

    // =============================== //
    // ===========  WRITE  =========== //
    pub fn value(&mut self) -> f64 {
        match self.valid {
            true => self.storage,
            false => {
                self.storage = if self.add.is_some() {
                    self.add.as_ref().unwrap().value()
                } else {
                    1.0
                };
                self.valid = true;

                self.storage
            }
        }
    }

    pub fn remove(&mut self, index: usize) {
        self.valid = false;

        match &mut self.add {
            Some(add) => add.remove(index),
            None => (),
        }
    }

    pub fn add(&mut self, mul: Mul) {
        self.valid = false;

        match &mut self.add {
            Some(add) => add.add(mul),
            None => {
                self.add = Some(Add::empty_new());
                self.add.as_mut().unwrap().add(mul);
            }
        }
    }

    pub fn collect(&mut self, index: usize) {
        unimplemented!()
    }

    // pub fn collect(&mut self, index: usize) -> Vec<Mul> {
    //     match &mut self.add {
    //         Some(add) => add.collect(index),
    //         None => vec![],
    //     }
    // }

    pub fn disperse(&mut self, index: usize) {
        match &mut self.add {
            Some(add) => add.disperse(index),
            None => (),
        }
    }
}


pub fn allocate(memory: &mut Memory, add: Option<Add>) -> usize {
    memory.insert_new(memory.len(), MemoryCell { storage: -1.0, valid: false, add });
    memory.len() - 1
}


#[cfg(test)]
mod tests {

    use super::*;
    use crate::circuit::rc::RC;
    use crate::circuit::leaf::Leaf;

    #[test]
    fn test_memory() {
        // Create foliage and basic memory layour
        let foliage = Arc::new(Mutex::new(vec![Leaf::new(&0.5, &0.0, "a"), Leaf::new(&0.5, &0.0, "b")]));
        let rc = RC::new(foliage.clone());

        // Test memory properties after RC initialization
        // Both should be set as valid ...
        assert_eq!(rc.memory.get(&0).unwrap().valid, true);
        assert_eq!(rc.memory.get(&1).unwrap().valid, true);
        // and contain a 0.0 (RC value) and 1.0 (const 1.0 for Mul without sub-circuit)
        assert_eq!(rc.memory.get_mut(&0).unwrap().value(), 0.0);
        assert_eq!(rc.memory.get_mut(&1).unwrap().value(), 1.0);
    }

}
