use core::panic;
use std::collections::BTreeSet;
use std::ops;
use std::sync::MutexGuard;

use super::add::Add;
use super::leaf::Leaf;
use super::memory::Memory;

#[derive(Clone)]
pub struct Mul {
    pub scope: BTreeSet<usize>,
    pub factors: BTreeSet<usize>,
    pub memory: Option<Memory>,
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
    pub fn new(factors: Vec<usize>) -> Self {
        // Ensure sorted indices
        let factors = BTreeSet::from_iter(factors);

        // Scope is the sorted, unique set of referenced leafs
        let scope = BTreeSet::from_iter(factors.iter().copied());

        Self {
            scope,
            factors,
            memory: None,
        }
    }

    pub fn empty_new() -> Self {
        Self {
            scope: BTreeSet::new(),
            factors: BTreeSet::new(),
            memory: None,
        }
    }

    // ============================== //
    // ===========  READ  =========== //
    pub fn value(&mut self, foliage_guard: &MutexGuard<Vec<Leaf>>) -> f64 {
        // Obtain all relevant leafs
        let leafs: Vec<&Leaf> = self
            .factors
            .iter()
            .map(|index| &foliage_guard[*index])
            .collect();

        // Compute overall product
        leafs
            .iter()
            .fold(self.memory_value(&foliage_guard), |acc, leaf| {
                acc * leaf.get_value()
            })
    }

    pub fn counted_value(&mut self, foliage_guard: &MutexGuard<Vec<Leaf>>) -> (f64, usize) {
        // Obtain all relevant leafs
        let leafs: Vec<&Leaf> = self
            .factors
            .iter()
            .map(|index| &foliage_guard[*index])
            .collect();

        // Compute overall product
        leafs.iter().fold(
            self.memory_counted_value(&foliage_guard),
            |mut acc, leaf| {
                acc.0 *= leaf.get_value();
                acc.1 += 1;
                acc
            },
        )
    }

    pub fn memory_value(&mut self, foliage_guard: &MutexGuard<Vec<Leaf>>) -> f64 {
        match &mut self.memory {
            Some(memory) => memory.value(&foliage_guard),
            None => 1.0,
        }
    }

    pub fn memory_counted_value(&mut self, foliage_guard: &MutexGuard<Vec<Leaf>>) -> (f64, usize) {
        match &mut self.memory {
            Some(memory) => memory.counted_value(&foliage_guard),
            None => (1.0, 0),
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

    pub fn update_dependencies(&self, foliage_guard: &mut MutexGuard<Vec<Leaf>>) {
        match &self.memory {
            Some(memory) => {
                for index in &memory.add.scope {
                    foliage_guard[*index].add_dependency(memory.valid.clone());
                }

                memory.update_dependencies(foliage_guard);
            }
            None => (),
        }
    }

    pub fn get_dot_text(
        &self,
        index: Option<usize>,
        foliage_guard: &MutexGuard<Vec<Leaf>>,
    ) -> (String, usize) {
        let mut dot_text = String::new();
        let index = if index.is_some() { index.unwrap() } else { 0 };

        dot_text += &format!("p_{index} [label=\"&times;\"]\n");
        for factor in &self.factors {
            let name = foliage_guard[*factor].name.to_owned();
            dot_text += &format!("p_{index} -> {name}\n");
        }

        let mut last = index;
        let sub_text;
        match &self.memory {
            Some(memory) => {
                let next = index + 1;
                dot_text += &format!("p_{index} -> m_{next}\n");
                (sub_text, last) = memory.get_dot_text(Some(next), foliage_guard);
                dot_text += &sub_text;
            }
            None => (),
        }

        (dot_text, last)
    }

    // =============================== //
    // ===========  WRITE  =========== //
    pub fn mul_index(&mut self, index: usize) {
        self.scope.insert(index);
        self.factors.insert(index);
    }

    pub fn mul_add(&mut self, add: Add) {
        self.scope.extend(&add.scope);
        self.memory = Some(Memory::new(-1.0, false, Some(add)));
    }

    pub fn remove(&mut self, index: usize) {
        self.scope.remove(&index);
        self.factors.remove(&index);
    }

    pub fn collect(&mut self, index: usize, active: bool) -> Option<Collection> {
        // This mul directly factors over the leaf
        if active {
            if self.factors.contains(&index) {
                if self.factors.len() == 1 && self.memory.is_none() {
                    self.remove(index);
                    Some(Collection::Forward(vec![MarkedMul::Singleton]))
                } else {
                    self.remove(index);
                    Some(Collection::Forward(vec![MarkedMul::InScope(self.clone())]))
                }
            } else {
                Some(Collection::Forward(vec![MarkedMul::OutOfScope(
                    self.clone(),
                )]))
            }
        } else {
            match &mut self.memory {
                Some(memory) => match memory.collect(index) {
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
        if self.factors.remove(&index) {
            match &mut self.memory {
                Some(memory) => memory.mul_index(index),
                None => {
                    let factors = vec![index];
                    let inner_add = Add::from_mul(Mul::new(factors));
                    self.memory = Some(Memory::new(-1.0, false, Some(inner_add)));
                }
            }
        } else {
            match &mut self.memory {
                Some(memory) => memory.disperse(index),
                None => (),
            }
        }
    }
}

impl ops::Mul<usize> for Mul {
    type Output = Mul;

    fn mul(self, index: usize) -> Mul {
        let mut mul = self;
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
        let mut mul = Mul::new(vec![0, 1]);
        assert_eq!(mul.value(&rc.foliage.lock().unwrap()), 0.25);

        // Scope of mul needs to be all leafs and sorted
        assert_eq!(mul.scope, BTreeSet::from_iter(vec![0, 1]));

        // We should be able to removeide and multiply with leaf indices
        mul.remove(0);
        assert_eq!(mul.value(&rc.foliage.lock().unwrap()), 0.5);
        assert_eq!(mul.scope, BTreeSet::from_iter(vec![1]));
    }
}
