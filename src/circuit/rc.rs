use std::sync::Arc;
use chashmap::CHashMap;

use super::leaf::Foliage;
use super::add::Add;
use super::mul::Mul;
use super::memory::{Memory, MemoryCell};


pub struct RC {
    pub scope: Vec<usize>,
    pub memory: Memory,
    pub foliage: Foliage,
}


impl RC {
    // ============================= //
    // ========  CONSTRUCT  ======== //
    pub fn new(foliage: Foliage) -> Self {
        // We create two initial memory cells
        // - The 0th cell contains the RC value
        // - The 1st cell contains a const 1 for terminal products
        let cell_0 = MemoryCell {
            storage: 0.0,
            valid: true,
            add: Some(Add::empty_new()),
        };
        let cell_1 = MemoryCell {
            storage: 1.0,
            valid: true,
            add: None,
        };

        let map = CHashMap::new();
        map.insert_new(0, cell_0);
        map.insert_new(1, cell_1);

        Self {
            scope: vec![],
            memory: Arc::new(map),
            foliage: foliage.clone(),
        }
    }

    // ============================== //
    // ===========  READ  =========== //
    pub fn value(&self) -> f64 {
        // Obtain memorized value
        let cell = &mut self.memory.get_mut(&0).unwrap();
        cell.value()
    }

    // =============================== //
    // ===========  WRITE  =========== //
    pub fn add(&mut self, mul: Mul) {
        let mut memory_guard = self.memory.get_mut(&0).unwrap();
        memory_guard.add(mul);
    }

    pub fn remove(&mut self, index: usize) {
        let mut memory_guard = self.memory.get_mut(&0).unwrap();
        memory_guard.remove(index);
    }

    pub fn collect(&mut self, index: usize) {
        unimplemented!()
        // if self.scope.contains(&index) {
        //     let cell = &mut self.memory.lock().unwrap()[0];
        //     cell.collect(index);
        // }
    }

    pub fn disperse(&mut self, index: usize) {
        if self.scope.contains(&index) {
            let cell = &mut self.memory.get_mut(&0).unwrap();
            cell.disperse(index);
        }
    }
}


#[cfg(test)]
mod tests {

    use super::*;
    use crate::circuit::leaf::Leaf;
    use std::sync::Mutex;

    #[test]
    fn test_rc() {
        // Create foliage and basic memory layour
        let foliage = Arc::new(Mutex::new(vec![Leaf::new(&0.5, &0.0, "a"), Leaf::new(&0.5, &0.0, "b")]));
        let mut rc = RC::new(foliage.clone());

        // Empty RC should return 0
        assert_eq!(rc.value(), 0.0);

        // Mul should have value 0.5 * 0.5 = 0.25
        let mul = Mul::new(vec![0, 1], foliage.clone(), rc.memory.clone());
        rc.add(mul.clone());
        assert_eq!(mul.value(), rc.value());

        // We should be able to remove and thereby (potentially) divide the value
        rc.remove(0);
        assert_eq!(rc.value(), 0.5);

        // Dispersing should not change the value
        let value_before = rc.value();
        rc.disperse(0);
        assert_eq!(value_before, rc.value());

        // Collecting should not change the value
        // rc.collect(0);
        // assert_eq!(value_before, rc.value());
    }

}
