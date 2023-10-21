use std::sync::{Arc, Mutex};
use chashmap::CHashMap;

use super::leaf::Leaf;

pub type Foliage = Arc<Mutex<Vec<Leaf>>>;
pub type Memory = Arc<CHashMap<usize, MemoryCell>>;

#[derive(Clone)]
pub struct Add {
    pub scope: Vec<usize>,
    pub products: Vec<Mul>,
}

#[derive(Clone)]
pub struct Mul {
    pub scope: Vec<usize>,
    pub foliage_indices: Vec<usize>,
    pub memory_index: usize,
    pub foliage: Foliage,
    pub memory: Memory,
}

pub struct MemoryCell {
    pub storage: f64,
    pub valid: bool,
    pub add: Option<Add>,
}

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
            add: None,
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
        match &mut memory_guard.add {
            Some(add) => add.add(mul),
            None => {
                let mut add = Add::empty_new();
                add.add(mul);
                memory_guard.add = Some(add);
                memory_guard.valid = false;
            }
        }
    }

    // pub fn collect(&mut self, index: usize) {
    //     if self.scope.contains(&index) {
    //         let cell = &mut self.memory.lock().unwrap()[0];
    //         cell.collect(index);
    //     }
    // }

    pub fn disperse(&mut self, index: usize) {
        if self.scope.contains(&index) {
            let cell = &mut self.memory.get_mut(&0).unwrap();
            cell.disperse(index);
        }
    }
}

impl Add {
    // ============================= //
    // ========  CONSTRUCT  ======== //
    pub fn new(scope: Vec<usize>, products: Vec<Mul>) -> Self {
        Self {
            scope,
            products,
        }
    }

    pub fn empty_new() -> Self {
        Self { scope: vec![], products: vec![] }
    }

    // ============================== //
    // ===========  READ  =========== //
    pub fn value(&self) -> f64 {
        // Accumulate sum over inner products
        self.products.iter().fold(0.0, |acc, mul| acc + mul.value())
    }

    // =============================== //
    // ===========  WRITE  =========== //
    pub fn add(&mut self, mul: Mul) {
        // Obtain new scope for this Add
        self.scope.append(&mut mul.scope.clone());
        self.scope.sort();
        self.scope.dedup();

        // Move to own products
        self.products.push(mul);
    }

    pub fn div(&mut self, index: usize) {
        let position = self.scope.iter().position(|i| &index != i);
        match position {
            Some(i) => {
                self.scope.swap_remove(i);
                let _ = self
                    .products
                    .iter_mut()
                    .filter(|mul| mul.scope.contains(&index))
                    .map(|mul| mul.div(index));
            }
            None => (),
        }
    }

    // pub fn collect(&mut self, index: usize) -> Vec<Mul> {
    //     // Newly constructed Mul structures
    //     let mut collected_muls = vec![];

    //     // We found all relevant Mul objects
    //     let mut collected = vec![];
    //     let mut to_be_removed = vec![];
    //     for (i, product) in self.products.iter().enumerate() {
    //         // Continue if leaf is not in this product
    //         if !product.foliage_indices.contains(&index) {
    //             continue;
    //         }

    //         // Collect this product and its index
    //         collected.push(product);
    //         to_be_removed.push(i);
    //     }

    //     // Leaf is in scope but we need to go deeper
    //     if collected.is_empty() {
    //         for (i, mul) in self.products.iter().enumerate() {
    //             let replacements = vec![]
    //         }
    //         for new_mul in self.products.iter_mut().map(|mul| mul.collect(index)).collect() {
    //             self.add(new_mul);
    //         }
    //     }
    //     // We found the leaf in Mul instances
    //     else {
    //         for product in &mut collected {
    //             // Setup everything for new circuit structure underneath cell
    //             // let scope = product.scope.clone();

    //             // This is only the leaf itself
    //             if product.scope.len() == 1 {
    //                 collected_muls.push(**product);
    //                 continue;
    //             } 
                
    //             // Remove leaf from product
    //             product.div(index);


    //             let storage = product.value();
    //             let foliage_indices = vec![index];

    //             // Ensure that we are the only ones to access memory and foliage here
    //             let mut memory_guard = product.memory.lock().unwrap();
    //             let memory_index = memory_guard.len();

    //             // Setup a new Mul
    //             collected_muls.push(**product);
    //             let add = Some(Add::new(
    //                 product.scope.clone(),
    //                 vec![product.clone()],
    //             ));
    //             let cell = MemoryCell {
    //                 storage,
    //                 valid: true,
    //                 add,
    //             };

    //             memory_guard.push(cell);
    //         }
    //     }

    //     to_be_removed.iter().map(|i| self.products.swap_remove(*i));

    //     collected_muls
    // }

    pub fn disperse(&mut self, index: usize) {
        let _ = self
            .products
            .iter_mut()
            .filter(|mul| mul.scope.contains(&index))
            .map(|mul| mul.disperse(index));
    }
}

impl Mul {
    // ============================= //
    // ========  CONSTRUCT  ======== //
    pub fn new(
        scope: Vec<usize>,
        foliage_indices: Vec<usize>,
        foliage: Foliage,
        memory: Memory,
    ) -> Self {
        let memory_index = 1;

        Self {
            scope,
            foliage_indices,
            memory_index,
            foliage,
            memory: memory.clone(),
        }
    }

    // ============================== //
    // ===========  READ  =========== //
    pub fn value(&self) -> f64 {
        // Obtain all relevant leafs
        let foliage_guard = self.foliage.lock().unwrap();
        let leafs = self
            .foliage_indices
            .iter()
            .map(|index| &foliage_guard[*index]);

        // Obtain memorized value
        let cell = &mut self.memory.get_mut(&self.memory_index).unwrap();
        let cell_value = cell.value();
        drop(cell);

        // Compute overall product
        let product = leafs.fold(cell_value, |acc, leaf| acc * leaf.get_value());
        product
    }

    // =============================== //
    // ===========  WRITE  =========== //
    pub fn div(&mut self, index: usize) {
        self.scope.retain(|i| &index != i);
        self.foliage_indices.retain(|i| &index != i);
        let cell = &mut self.memory.get_mut(&self.memory_index).unwrap();
        cell.div(index);
    }

    // pub fn collect(&mut self, index: usize) -> Vec<Mul> {
    //     // This mul directly factors over the leaf
    //     if self.foliage_indices.contains(&index) {
    //         // And it is only the leaf with constant 1 cell
    //         if self.scope.len() == 1 {
    //             return vec![*self];
    //         }

    //         // Else we remove the leaf and 
    //         let scope = self.scope.clone();
    //         self.div(index);

    //         let memory_index = allocate(self.memory, Add::new(self.scope, self, self.memory));

    //         // let mul = Mul::new(scope, vec![index], memory_index, self.foliage.clone(), self.memory.clone());
    //         return vec![mul];
    //     }

    //     // Leaf is in scope but search continues downwards
    //     let cell = &mut self.memory.lock().unwrap()[self.memory_index];
    //     cell.collect(index)
    // }

    pub fn disperse(&mut self, index: usize) {
        let position = self.foliage_indices.iter().position(|i| &index == i);
        match position {
            Some(i) => {
                // Remove from foliage reference
                self.foliage_indices.swap_remove(i);

                // If this is pointing at const. 1, we need to create a new memory cell
                // and the structures underneath
                if self.memory_index == 1 {
                    // Ensure that we are the only ones to access memory and foliage here
                    let foliage_guard = self.foliage.lock().unwrap();


                    // Setup everything for new circuit structure underneath cell
                    let storage = foliage_guard[index].get_value();
                    let scope = vec![index];
                    let foliage_indices = vec![index];

                    // Setup single add over single mul of leaf and const 1
                    let products = vec![Mul::new(
                        scope.clone(),
                        foliage_indices,
                        self.foliage.clone(),
                        self.memory.clone(),
                    )];
                    // self.memory.lock().unwrap()[memory_index].add = Some(Add::new(scope, products));
                } else {
                    // Else we can just forward the dispersion to the next cell
                    let cell = &mut self.memory.get_mut(&self.memory_index).unwrap();
                    cell.disperse(index);
                }
            }
            None => (),
        }
    }
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

    pub fn div(&mut self, index: usize) {
        match &mut self.add {
            Some(add) => add.div(index),
            None => (),
        }
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

    #[test]
    fn test_adder() {
        // Create foliage and basic memory layour
        let foliage = Arc::new(Mutex::new(vec![Leaf::new(&0.5, &0.0, "a"), Leaf::new(&0.5, &0.0, "b")]));
        let rc = RC::new(foliage.clone());

        // Empty adder should return 0
        let mut add = Add::empty_new();
        assert_eq!(add.value(), 0.0);
        
        // Add over single mul should return result of mul
        let mul = Mul::new(vec![0, 1], vec![0, 1], foliage.clone(), rc.memory.clone());
        add.add(mul.clone());
        assert_eq!(mul.value(), add.value());
    }

    #[test]
    fn test_mul() {
        // Create foliage and basic memory layour
        let foliage = Arc::new(Mutex::new(vec![Leaf::new(&0.5, &0.0, "a"), Leaf::new(&0.5, &0.0, "b")]));
        let rc = RC::new(foliage.clone());
        
        // Mul should have value 0.5 * 0.5 = 0.25
        let mul = Mul::new(vec![0, 1], vec![0, 1], foliage.clone(), rc.memory.clone());
        assert_eq!(mul.value(), 0.25);

        // Mul should point at cell 1 with value 1.0
        assert_eq!(mul.memory_index, 1);
    }

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

    #[test]
    fn test_rc() {
        // Create foliage and basic memory layour
        let foliage = Arc::new(Mutex::new(vec![Leaf::new(&0.5, &0.0, "a"), Leaf::new(&0.5, &0.0, "b")]));
        let mut rc = RC::new(foliage.clone());

        // Empty RC should return 0
        assert_eq!(rc.value(), 0.0);

        // // Mul should have value 0.5 * 0.5 = 0.25
        let mul = Mul::new(vec![0, 1], vec![0, 1], foliage.clone(), rc.memory.clone());
        rc.add(mul.clone());
        assert_eq!(mul.value(), rc.value());
    }
}