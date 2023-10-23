use std::ops;

use super::leaf::Foliage;
use super::memory::Memory;
use super::mul::Collection;
use super::mul::Mul;

#[derive(Clone)]
pub struct Add {
    pub scope: Vec<usize>,
    pub products: Vec<Mul>,
    pub foliage: Foliage,
    pub memory: Memory,
}

impl Add {
    // ============================= //
    // ========  CONSTRUCT  ======== //
    pub fn new(scope: Vec<usize>, products: Vec<Mul>, foliage: Foliage, memory: Memory) -> Self {
        Self {
            scope,
            products,
            foliage,
            memory,
        }
    }

    pub fn empty_new(foliage: Foliage, memory: Memory) -> Self {
        Self {
            scope: vec![],
            products: vec![],
            foliage,
            memory,
        }
    }

    pub fn from_index_matrix(
        index_matrix: Vec<Vec<usize>>,
        foliage: Foliage,
        memory: Memory,
    ) -> Self {
        // Fill new Add structure with products and scope
        let mut add = Add::empty_new(foliage.clone(), memory.clone());
        for leaf_indices in index_matrix {
            add.scope.append(&mut leaf_indices.to_owned());
            add.products
                .push(Mul::new(leaf_indices, foliage.clone(), memory.clone()));
        }

        // Correct scope to only contain the unique elements
        add.scope.sort();
        add.scope.dedup();

        add
    }

    pub fn from_mul(mul: Mul) -> Add {
        let mut add = Add::empty_new(mul.foliage.clone(), mul.memory.clone());
        add.products.push(mul);
        add
    }

    // ============================== //
    // ===========  READ  =========== //
    pub fn value(&self) -> f64 {
        // Accumulate sum over inner products
        self.products.iter().fold(0.0, |acc, mul| acc + mul.value())
    }

    pub fn flat(&self, memory: &Memory) -> Add {
        let flattened_products: Vec<Mul> = self
            .products
            .iter()
            .map(|mul| mul.flat(memory))
            .flatten()
            .collect();
        let mut flat_add = Add::empty_new(self.foliage.clone(), memory.clone());
        for mul in flattened_products {
            flat_add.add_mul(mul);
        }

        flat_add
    }

    pub fn is_flat(&self) -> bool {
        self.products.iter().all(|mul| mul.is_flat())
    }

    pub fn is_equal(&self, other: &Add) -> bool {
        // Check scope and overall number of products
        if self.scope != other.scope || self.products.len() != other.products.len() {
            return false;
        }

        // Check if each product below this add is matched by other
        let mut candidates = other.products.clone();
        for mul in &self.products {
            let position = candidates
                .iter()
                .position(|candidate| mul.is_equal(candidate));
            match position {
                Some(position) => {
                    candidates.swap_remove(position);
                    continue;
                }
                None => {
                    return false;
                }
            }
        }

        true
    }

    // =============================== //
    // ===========  WRITE  =========== //
    pub fn add_mul(&mut self, mul: Mul) {
        // Obtain new scope for this Add
        self.scope.append(&mut mul.scope.clone());
        self.scope.sort();
        self.scope.dedup();

        // Move to own products
        self.products.push(mul);
    }

    pub fn remove(&mut self, index: usize) {
        let position = self.scope.iter().position(|i| &index != i);
        match position {
            Some(i) => {
                self.scope.swap_remove(i);
                let _ = self
                    .products
                    .iter_mut()
                    .filter(|mul| mul.scope.contains(&index))
                    .for_each(|mul| mul.remove(index));
            }
            None => (),
        }
    }

    pub fn collect(&mut self, index: usize) -> Option<Collection> {
        let mut is_apply = false;
        let mut is_forward = false;
        let mut collection = vec![];
        let mut to_remove = vec![];
        for i in 0..self.products.len() {
            match self.products[i].collect(index) {
                Some(Collection::Apply(mut muls)) => {
                    collection.append(&mut muls);
                    is_apply = true;
                }
                Some(Collection::Forward(mut muls)) => {
                    collection.append(&mut muls);
                    to_remove.push(i);
                    is_forward = true;
                }
                None => continue
            }
        }

        // Sanity check that leafs are all on single layer
        if is_apply && is_forward {
            panic!("A leaf is distributed over multiple layers!");
        }

        // Remove products that have generated a Forward
        to_remove.iter().for_each(|i| { self.products.swap_remove(*i); });

        // Pass gathered collections up
        if collection.is_empty() {
            None
        } else if is_apply {
            self._apply_collection(index, collection);
            None
        } else {
            Some(Collection::Forward(collection))
        }
    }

    pub fn _apply_collection(&mut self, index: usize, collection: Vec<Mul>) {
        for mul in &collection {
            if mul.scope.is_empty() {
                self.add_mul(Mul::new(vec![index], self.foliage.clone(), self.memory.clone()));
            } else {
                self.add_mul(Add::from_mul(mul.clone()) * index);
            }
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
    //         if !product.leaf_indices.contains(&index) {
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
    //             product.remove(index);

    //             let storage = product.value();
    //             let leaf_indices = vec![index];

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
            .for_each(|mul| mul.disperse(index));
    }
}


impl ops::Mul<usize> for Add {
    type Output = Mul;

    fn mul(self, index: usize) -> Mul {
        let mut mul = Mul::empty_new(self.foliage.clone(), self.memory.clone());
        
        mul.mul_add(self.clone());
        mul.mul_index(index);

        mul
    }
}

impl ops::Add<Add> for Add {
    type Output = Add;

    fn add(self, other: Add) -> Add {
        let mut add = Add::empty_new(self.foliage.clone(), self.memory.clone());
        
        self.products.iter().for_each(|mul| add.add_mul(mul.clone()));
        other.products.iter().for_each(|mul| add.add_mul(mul.clone()));

        add
    }
}

#[cfg(test)]
mod tests {

    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::circuit::leaf::Leaf;
    use crate::circuit::rc::RC;

    #[test]
    fn test_add() {
        // Create foliage and basic memory layour
        let foliage = Arc::new(Mutex::new(vec![
            Leaf::new(&0.5, &0.0, "a"),
            Leaf::new(&0.5, &0.0, "b"),
        ]));
        let rc = RC::new(foliage.clone());

        // Empty adder should return 0
        let mut add = Add::empty_new(rc.foliage.clone(), rc.memory.clone());
        assert_eq!(add.value(), 0.0);

        // Add over single mul should return result of mul
        let mul = Mul::new(vec![0, 1], foliage.clone(), rc.memory.clone());
        add.add_mul(mul.clone());
        assert_eq!(mul.value(), add.value());
    }
}
