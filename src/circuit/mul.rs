use super::leaf::Foliage;
use super::memory::Memory;


#[derive(Clone)]
pub struct Mul {
    pub scope: Vec<usize>,
    pub leaf_indices: Vec<usize>,
    pub memory_index: usize,
    pub foliage: Foliage,
    pub memory: Memory,
}


impl Mul {
    // ============================= //
    // ========  CONSTRUCT  ======== //
    pub fn new(
        leaf_indices: Vec<usize>,
        foliage: Foliage,
        memory: Memory,
    ) -> Self {
        let memory_index = 1;

        Self {
            scope: leaf_indices.clone(),
            leaf_indices,
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
            .leaf_indices
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
    pub fn remove(&mut self, index: usize) {
        self.scope.retain(|i| &index != i);
        self.leaf_indices.retain(|i| &index != i);
        let cell = &mut self.memory.get_mut(&self.memory_index).unwrap();
        cell.remove(index);
    }

    pub fn collect(&mut self, index: usize) {
        unimplemented!()
    }

    // pub fn collect(&mut self, index: usize) -> Vec<Mul> {
    //     // This mul directly factors over the leaf
    //     if self.leaf_indices.contains(&index) {
    //         // And it is only the leaf with constant 1 cell
    //         if self.scope.len() == 1 {
    //             return vec![*self];
    //         }

    //         // Else we remove the leaf and 
    //         let scope = self.scope.clone();
    //         self.remove(index);

    //         let memory_index = allocate(self.memory, Add::new(self.scope, self, self.memory));

    //         // let mul = Mul::new(scope, vec![index], memory_index, self.foliage.clone(), self.memory.clone());
    //         return vec![mul];
    //     }

    //     // Leaf is in scope but search continues downwards
    //     let cell = &mut self.memory.lock().unwrap()[self.memory_index];
    //     cell.collect(index)
    // }

    pub fn disperse(&mut self, index: usize) {
        let position = self.leaf_indices.iter().position(|i| &index == i);
        match position {
            Some(i) => {
                // Remove from foliage reference
                self.leaf_indices.swap_remove(i);

                // If this is pointing at const. 1, we need to create a new memory cell
                // and the structures underneath
                if self.memory_index == 1 {
                    // Ensure that we are the only ones to access memory and foliage here
                    let foliage_guard = self.foliage.lock().unwrap();


                    // Setup everything for new circuit structure underneath cell
                    let storage = foliage_guard[index].get_value();
                    let scope = vec![index];
                    let leaf_indices = vec![index];

                    // Setup single add over single mul of leaf and const 1
                    let products = vec![Mul::new(
                        leaf_indices,
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


#[cfg(test)]
mod tests {

    use super::*;
    use crate::circuit::leaf::Leaf;
    use crate::circuit::rc::RC;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_mul() {
        // Create foliage and basic memory layour
        let foliage = Arc::new(Mutex::new(vec![Leaf::new(&0.5, &0.0, "a"), Leaf::new(&0.5, &0.0, "b")]));
        let rc = RC::new(foliage.clone());
        
        // Mul should have value 0.5 * 0.5 = 0.25
        let mut mul = Mul::new(vec![0, 1], foliage.clone(), rc.memory.clone());
        assert_eq!(mul.value(), 0.25);

        // Mul should point at cell 1 with value 1.0
        assert_eq!(mul.memory_index, 1);

        // We should be able to removeide and multiply with leaf indices
        mul.remove(0);
        assert_eq!(mul.value(), 0.5);
    }

}
