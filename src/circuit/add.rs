use super::leaf::{Leaf, Foliage};
use super::mul::Mul;
use super::rc::RC;

#[derive(Clone)]
pub struct Add {
    pub scope: Vec<usize>,
    pub products: Vec<Mul>,
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

    pub fn collect(&mut self, index: usize) {
        unimplemented!()
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


#[cfg(test)]
mod tests {

    use super::*;
    use crate::circuit::rc::RC;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_add() {
        // Create foliage and basic memory layour
        let foliage = Arc::new(Mutex::new(vec![Leaf::new(&0.5, &0.0, "a"), Leaf::new(&0.5, &0.0, "b")]));
        let rc = RC::new(foliage.clone());

        // Empty adder should return 0
        let mut add = Add::empty_new();
        assert_eq!(add.value(), 0.0);
        
        // Add over single mul should return result of mul
        let mul = Mul::new(vec![0, 1], foliage.clone(), rc.memory.clone());
        add.add(mul.clone());
        assert_eq!(mul.value(), add.value());
    }

}
