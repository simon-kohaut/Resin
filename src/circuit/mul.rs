use core::panic;
use std::collections::BTreeSet;
use std::ops;
use std::sync::MutexGuard;

use rayon::iter::ParallelIterator;
use rayon::prelude::*;

use super::add::Add;
use super::leaf::Leaf;
use super::memory::Memory;

#[derive(Clone)]
pub struct Mul {
    pub scope: BTreeSet<u16>,
    pub factors: BTreeSet<u16>,
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
    pub fn new(factors: Vec<u16>) -> Self {
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
        let value = if self.memory.is_some() {
            self.memory.as_mut().unwrap().value(&foliage_guard)
        } else {
            1.0
        };
        self.factors
            .iter()
            .map(|index| foliage_guard[*index as usize].get_value())
            .product::<f64>()
            * value
    }

    pub fn counted_value(&mut self, foliage_guard: &MutexGuard<Vec<Leaf>>) -> (f64, usize) {
        let (mut value, mut count) = if self.memory.is_some() {
            self.memory.as_mut().unwrap().counted_value(&foliage_guard)
        } else {
            (1.0, 0)
        };
        value *= self
            .factors
            .iter()
            .map(|index| foliage_guard[*index as usize].get_value())
            .product::<f64>();

        count += self.factors.len();

        (value, count)
    }

    pub fn is_flat(&self) -> bool {
        self.memory.is_none()
    }

    pub fn is_equal(&self, other: &Mul) -> bool {
        self.scope == other.scope && self.factors == other.factors
    }

    pub fn update_dependencies(&self, foliage_guard: &mut MutexGuard<Vec<Leaf>>) {
        if self.memory.is_some() {
            let memory = self.memory.as_ref().unwrap();

            for index in &memory.add.scope {
                foliage_guard[*index as usize].add_dependency(memory.valid.clone());
            }

            memory.update_dependencies(foliage_guard);
        }
    }

    pub fn count_adds(&self) -> usize {
        match &self.memory {
            Some(memory) => memory.count_adds(),
            None => 0,
        }
    }

    pub fn count_muls(&self) -> usize {
        match &self.memory {
            Some(memory) => memory.count_muls(),
            None => 0,
        }
    }

    pub fn layers(&self) -> usize {
        match &self.memory {
            Some(memory) => 1 + memory.layers(),
            None => 1,
        }
    }

    pub fn get_dot_text(
        &self,
        index: Option<u16>,
        foliage_guard: &MutexGuard<Vec<Leaf>>,
    ) -> (String, u16) {
        let mut dot_text = String::new();
        let index = if index.is_some() { index.unwrap() } else { 0 };

        let scope = &self.scope;
        dot_text += &format!("p_{index} [label=\"&times;\n{scope:?}\"]\n");
        for factor in &self.factors {
            let name = foliage_guard[*factor as usize].name.to_owned();
            dot_text += &format!("p_{index} -> {name}\n");
        }

        let mut last = index;
        let sub_text;
        if self.memory.is_some() {
            let memory = self.memory.as_ref().unwrap();
            let next = index + 1;

            dot_text += &format!("p_{index} -> m_{next}\n");
            (sub_text, last) = memory.get_dot_text(Some(next), foliage_guard);
            dot_text += &sub_text;
        }

        (dot_text, last)
    }

    // =============================== //
    // ===========  WRITE  =========== //
    pub fn mul_index(&mut self, index: u16) {
        self.scope.insert(index);
        self.factors.insert(index);
    }

    pub fn mul_add(&mut self, add: Add) {
        self.scope.extend(&add.scope);
        self.memory = Some(Memory::new(-1.0, false, Some(add)));
    }

    pub fn remove(&mut self, index: u16) {
        self.scope.remove(&index);
        self.factors.remove(&index);
    }

    pub fn collect(&mut self, index: u16, active: bool, repeat: usize) -> Option<Collection> {
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
        } else if self.memory.is_some() {
            match self.memory.as_mut().unwrap().collect(index, repeat) {
                Some(Collection::Forward(_)) => {
                    panic!("MemoryCells should only return Collection::Apply!")
                }
                Some(Collection::Apply(muls)) => Some(Collection::Apply(muls)),
                None => None,
            }
        } else {
            None
        }
    }

    pub fn disperse(&mut self, index: u16, repeat: usize, value: f64) {
        if self.factors.remove(&index) {
            match &mut self.memory {
                Some(memory) => memory.mul_index(index, value),
                None => {
                    let factors = vec![index];
                    let inner_add = Add::from_mul(Mul::new(factors));
                    self.memory = Some(Memory::new(value, true, Some(inner_add)));
                }
            }

            if repeat > 0 {
                self.memory
                    .as_mut()
                    .unwrap()
                    .disperse(index, repeat - 1, value);
            }
        } else if self.memory.is_some() {
            self.memory.as_mut().unwrap().disperse(index, repeat, value);
        }
    }

    pub fn deploy(&self) -> Vec<Memory> {
        match &self.memory {
            Some(memory) => {
                let mut memories = vec![memory.clone()];
                memories.append(&mut memory.deploy());

                memories
            }
            None => vec![],
        }
    }

    pub fn empty_scope(&mut self) {
        self.scope.clear();
        if self.memory.is_some() {
            self.memory.as_mut().unwrap().empty_scope();
        }
    }
}

impl ops::Mul<u16> for Mul {
    type Output = Mul;

    fn mul(self, index: u16) -> Mul {
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
