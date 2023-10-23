use core::panic;
use std::ops;

use super::leaf::Foliage;
use super::add::Add;
use super::memory::{Memory, MemoryCell};

#[derive(Clone)]
pub struct Mul {
    pub scope: Vec<usize>,
    pub factors: Vec<usize>,
    pub memory_index: usize,
    pub foliage: Foliage,
    pub memory: Memory,
}

pub enum Collection {
    Forward(Vec<Mul>),
    Apply(Vec<Mul>),
}

impl Mul {
    // ============================= //
    // ========  CONSTRUCT  ======== //
    pub fn new(mut factors: Vec<usize>, foliage: Foliage, memory: Memory) -> Self {
        // Point at const. 1.0
        let memory_index = 1;

        // Ensure sorted indices
        factors.sort();

        // Scope is the sorted, unique set of referenced leafs
        let mut scope = factors.clone();
        scope.dedup();

        Self {
            scope,
            factors,
            memory_index,
            foliage,
            memory,
        }
    }

    pub fn empty_new(foliage: Foliage, memory: Memory) -> Self {
        Self {
            scope: vec![],
            factors: vec![],
            memory_index: 1,
            foliage,
            memory,
        }
    }

    // ============================== //
    // ===========  READ  =========== //
    pub fn value(&self) -> f64 {
        // Obtain all relevant leafs
        let foliage_guard = self.foliage.lock().unwrap();
        let leafs = self.factors.iter().map(|index| &foliage_guard[*index]);

        // Obtain memorized value
        let cell = &mut self.memory.get_mut(&self.memory_index).unwrap();
        let cell_value = cell.value();
        drop(cell);

        // Compute overall product
        let product = leafs.fold(cell_value, |acc, leaf| acc * leaf.get_value());
        product
    }

    pub fn flat(&self, memory: &Memory) -> Vec<Mul> {
        if self.is_flat() {
            vec![self.clone()]
        } else {
            let flat_add = self.memory.get(&self.memory_index).unwrap().flat(memory);
            match flat_add {
                Some(flat_add) => {
                    let mut flat_muls = vec![];
                    for mul in flat_add.products {
                        flat_muls.push(mul.factors.iter().fold(self.clone(), |mut acc, i| {
                            acc.mul_index(*i);
                            acc
                        }));
                    }

                    flat_muls
                }
                None => vec![self.clone()],
            }
        }
    }

    pub fn is_flat(&self) -> bool {
        // This is only flat if it points at the const. 1.0 cell
        self.memory_index == 1
    }

    pub fn is_equal(&self, other: &Mul) -> bool {
        self.scope == other.scope && self.factors == other.factors
    }

    // =============================== //
    // ===========  WRITE  =========== //
    pub fn mul_index(&mut self, index: usize) {
        // Obtain new scope for this Mul
        match self.scope.binary_search(&index) {
            Ok(_) => (),
            Err(position) => self.scope.insert(position, index),
        }

        // Extend factors
        let position = self.factors.binary_search(&index).unwrap();
        self.factors.insert(position, index);
    }

    pub fn mul_add(&mut self, add: Add) {
        let cell = MemoryCell {
            storage: 0.0,
            valid: false,
            add: Some(add),
            foliage: self.foliage.clone(),
            memory: self.memory.clone(),
        };
        self.memory_index = self.memory.len();
        self.memory.insert_new(self.memory_index, cell);
    }

    pub fn remove(&mut self, index: usize) {
        self.scope.retain(|i| &index != i);
        self.factors.retain(|i| &index != i);
        let cell = &mut self.memory.get_mut(&self.memory_index).unwrap();
        cell.remove(index);
    }

    pub fn collect(&mut self, index: usize) -> Option<Collection> {
        // This mul directly factors over the leaf
        if self.factors.contains(&index) {
            self.remove(index);
            Some(Collection::Forward(vec![self.clone()]))
        } else {
            match self
                .memory
                .get_mut(&self.memory_index)
                .unwrap()
                .collect(index)
            {
                Some(Collection::Forward(_)) => {
                    panic!("MemoryCells should only return Collection::Apply!")
                }
                Some(Collection::Apply(muls)) => Some(Collection::Apply(muls)),
                None => None,
            }
        }
    }

    pub fn disperse(&mut self, index: usize) {
        let position = self.factors.iter().position(|i| &index == i);
        match position {
            Some(i) => {
                // Remove from foliage reference
                self.factors.swap_remove(i);

                // If this is pointing at const. 1, we need to create a new memory cell
                // and the structures underneath
                if self.memory_index == 1 {
                    // Ensure that we are the only ones to access memory and foliage here
                    let foliage_guard = self.foliage.lock().unwrap();

                    // Setup everything for new circuit structure underneath cell
                    let storage = foliage_guard[index].get_value();
                    let scope = vec![index];
                    let factors = vec![index];

                    // Setup single add over single mul of leaf and const 1
                    let products =
                        vec![Mul::new(factors, self.foliage.clone(), self.memory.clone())];
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


impl ops::Mul<usize> for Mul {
    type Output = Mul;

    fn mul(self, index: usize) -> Mul {
        let mut mul = self.clone();
        mul.mul_index(index);
        mul
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
        let foliage = Arc::new(Mutex::new(vec![
            Leaf::new(&0.5, &0.0, "a"),
            Leaf::new(&0.5, &0.0, "b"),
        ]));
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
