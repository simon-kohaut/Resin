use core::panic;
use std::ops;
use std::sync::{Arc, Mutex};

use super::add::Add;
use super::leaf::Foliage;
use super::memory::Memory;

#[derive(Clone)]
pub struct Mul {
    pub scope: Vec<usize>,
    pub factors: Vec<usize>,
    pub foliage: Foliage,
    pub memory: Option<Arc<Mutex<Memory>>>,
}

pub enum Collection {
    Forward(Vec<MarkedMul>),
    Apply(Vec<MarkedMul>),
}

pub enum MarkedMul {
    Singleton,
    InScope(Mul),
    OutOfScope(Mul),
}

impl Mul {
    // ============================= //
    // ========  CONSTRUCT  ======== //
    pub fn new(mut factors: Vec<usize>, foliage: Foliage) -> Self {
        // Ensure sorted indices
        factors.sort();

        // Scope is the sorted, unique set of referenced leafs
        let mut scope = factors.clone();
        scope.dedup();

        Self {
            scope,
            factors,
            foliage,
            memory: None,
        }
    }

    pub fn empty_new(foliage: Foliage) -> Self {
        Self {
            scope: vec![],
            factors: vec![],
            foliage,
            memory: None,
        }
    }

    // ============================== //
    // ===========  READ  =========== //
    pub fn value(&self) -> f64 {
        // Obtain all relevant leafs
        let foliage_guard = self.foliage.lock().unwrap();
        let leafs = self.factors.iter().map(|index| &foliage_guard[*index]);

        // Compute overall product
        let product = leafs.fold(self.memory_value(), |acc, leaf| acc * leaf.get_value());
        product
    }

    pub fn memory_value(&self) -> f64 {
        match &self.memory {
            Some(memory) => memory.lock().unwrap().value(),
            None => 1.0,
        }
    }

    pub fn is_flat(&self) -> bool {
        match self.memory {
            Some(_) => false,
            None => true,
        }
    }

    pub fn is_equal(&self, other: &Mul) -> bool {
        self.scope == other.scope && self.factors == other.factors
    }

    pub fn get_dot_text(&self, index: Option<usize>) -> (String, usize) {
        let mut dot_text = String::new();
        let index = if index.is_some() { index.unwrap() } else { 0 };

        dot_text += &format!("p_{index} [label=\"&times;\"]\n",);
        for factor in &self.factors {
            let foliage_guard = self.foliage.lock().unwrap();
            let name = foliage_guard[*factor].name.to_owned();
            dot_text += &format!("p_{index} -> {name}\n");
        }

        let mut last = index;
        let sub_text;
        match &self.memory {
            Some(memory) => {
                let next = index + 1;
                dot_text += &format!("p_{index} -> m_{next}\n");
                (sub_text, last) = memory.lock().unwrap().get_dot_text(Some(next));
                dot_text += &sub_text;
            }
            None => (),
        }

        (dot_text, last)
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
        let position;
        match self.factors.binary_search(&index) {
            Ok(i) => position = i,
            Err(i) => position = i,
        }
        self.factors.insert(position, index);
    }

    pub fn mul_add(&mut self, add: Add) {
        self.scope.append(&mut add.scope.clone());
        self.scope.sort();
        self.scope.dedup();

        self.memory = Some(Arc::new(Mutex::new(Memory::new(
            -1.0,
            false,
            Some(add),
            self.foliage.clone(),
        ))));
    }

    pub fn remove(&mut self, index: usize) {
        if self.scope.contains(&index) {
            self.scope.retain(|i| &index != i);
            self.factors.retain(|i| &index != i);

            match &self.memory {
                Some(memory) => memory.lock().unwrap().remove(index),
                None => (),
            }
        }
    }

    pub fn collect(&mut self, index: usize, active: bool) -> Option<Collection> {
        // This mul directly factors over the leaf
        if active {
            if self.factors == vec![index] && self.memory.is_none() {
                self.remove(index);
                Some(Collection::Forward(vec![MarkedMul::Singleton]))
            } else if self.factors.contains(&index) {
                self.remove(index);
                Some(Collection::Forward(vec![MarkedMul::InScope(self.clone())]))
            } else {
                Some(Collection::Forward(vec![MarkedMul::OutOfScope(
                    self.clone(),
                )]))
            }
        } else {
            match &self.memory {
                Some(memory) => match memory.lock().unwrap().collect(index) {
                    Some(Collection::Forward(_)) => {
                        panic!("MemoryCells should only return Collection::Apply!")
                    }
                    Some(Collection::Apply(muls)) => Some(Collection::Apply(muls)),
                    None => None,
                },
                None => None,
            }
        }
    }

    pub fn disperse(&mut self, index: usize) {
        if self.factors.contains(&index) {
            self.factors.retain(|i| *i != index);

            match &self.memory {
                Some(memory) => memory.lock().unwrap().mul_index(index),
                None => {
                    let factors = vec![index];
                    let inner_add = Add::from_mul(Mul::new(factors, self.foliage.clone()));
                    self.memory = Some(Arc::new(Mutex::new(Memory::new(
                        -1.0,
                        false,
                        Some(inner_add),
                        self.foliage.clone(),
                    ))));
                }
            }
        } else {
            match &self.memory {
                Some(memory) => memory.lock().unwrap().disperse(index),
                None => (),
            }
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
    use crate::circuit::rc::RC;

    #[test]
    fn test_mul() {
        // Create basic RC
        let mut rc = RC::new();
        rc.grow(0.5, "a");
        rc.grow(0.5, "b");

        // Mul should have value 0.5 * 0.5 = 0.25
        let mut mul = Mul::new(vec![0, 1], rc.foliage.clone());
        assert_eq!(mul.value(), 0.25);

        // Scope of mul needs to be all leafs and sorted
        assert_eq!(mul.scope, vec![0, 1]);

        // We should be able to removeide and multiply with leaf indices
        mul.remove(0);
        assert_eq!(mul.value(), 0.5);
        assert_eq!(mul.scope, vec![1]);
    }
}
