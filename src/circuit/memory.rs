use chashmap::CHashMap;
use std::sync::Arc;

use super::add::Add;
use super::leaf::Foliage;
use super::mul::Collection;
use super::mul::Mul;

pub type Memory = Arc<CHashMap<usize, MemoryCell>>;

pub struct MemoryCell {
    pub storage: f64,
    pub valid: bool,
    pub add: Option<Add>,
    pub foliage: Foliage,
    pub memory: Memory,
}

impl MemoryCell {
    // ============================= //
    // ========  CONSTRUCT  ======== //
    pub fn new(
        storage: f64,
        valid: bool,
        add: Option<Add>,
        foliage: Foliage,
        memory: Memory,
    ) -> Self {
        Self {
            storage,
            valid,
            add,
            foliage,
            memory,
        }
    }

    pub fn new_one(foliage: Foliage, memory: Memory) -> Self {
        Self {
            storage: 1.0,
            valid: true,
            add: None,
            foliage,
            memory,
        }
    }

    // ============================== //
    // ===========  READ  =========== //
    pub fn flat(&self, memory: &Memory) -> Option<Add> {
        match &self.add {
            Some(add) => Some(add.flat(memory)),
            None => None,
        }
    }

    pub fn is_flat(&self) -> bool {
        match &self.add {
            Some(add) => add.is_flat(),
            None => true,
        }
    }

    pub fn is_equal(&self, other: &MemoryCell) -> bool {
        if self.add.is_none() && other.add.is_none() {
            true
        } else {
            self.add
                .as_ref()
                .unwrap()
                .is_equal(other.add.as_ref().unwrap())
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
            Some(add) => add.add_mul(mul),
            None => {
                self.add = Some(Add::empty_new(self.foliage.clone(), self.memory.clone()));
                self.add.as_mut().unwrap().add_mul(mul);
            }
        }
    }

    pub fn collect(&mut self, index: usize) -> Option<Collection> {
        match &mut self.add {
            Some(add) => {
                match add.collect(index) {
                    Some(Collection::Apply(_)) => panic!("MemoryCells should never get Collection::Apply!"),
                    Some(Collection::Forward(muls)) => Some(Collection::Apply(muls)),
                    None => None,
                }
            }
            None => None,
        }
    }

    pub fn disperse(&mut self, index: usize) {
        match &mut self.add {
            Some(add) => add.disperse(index),
            None => (),
        }
    }
}

#[cfg(test)]
mod tests {

    use std::sync::Mutex;

    use super::*;
    use crate::circuit::leaf::Leaf;
    use crate::circuit::rc::RC;

    #[test]
    fn test_memory() {
        // Create foliage and basic memory layour
        let foliage = Arc::new(Mutex::new(vec![
            Leaf::new(&0.5, &0.0, "a"),
            Leaf::new(&0.5, &0.0, "b"),
        ]));
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
