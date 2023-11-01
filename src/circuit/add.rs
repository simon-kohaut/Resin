use std::collections::BTreeSet;
use std::ops;
use std::sync::MutexGuard;

use rayon::iter::IndexedParallelIterator;
use rayon::prelude::*;

use super::leaf::Leaf;
use super::memory::Memory;
use super::mul::Collection;
use super::mul::MarkedMul;
use super::mul::Mul;

#[derive(Clone)]
pub struct Add {
    pub scope: BTreeSet<u16>,
    pub products: Vec<Mul>,
}

impl Add {
    // ============================= //
    // ========  CONSTRUCT  ======== //
    pub fn new(products: Vec<Mul>) -> Self {
        let mut scope = BTreeSet::new();
        products.iter().for_each(|mul| scope.extend(&mul.scope));

        Self { scope, products }
    }

    pub fn empty_new() -> Self {
        Self {
            scope: BTreeSet::new(),
            products: vec![],
        }
    }

    pub fn from_index_matrix(index_matrix: Vec<Vec<u16>>) -> Self {
        // Fill new Add structure with products and scope
        let mut add = Add::empty_new();
        for leaf_indices in index_matrix {
            add.scope.extend(&leaf_indices);
            add.products.push(Mul::new(leaf_indices));
        }

        add
    }

    pub fn from_mul(mul: Mul) -> Add {
        let mut add = Add::empty_new();
        add.add_mul(mul);
        add
    }

    // ============================== //
    // ===========  READ  =========== //
    #[inline(always)]
    pub fn value(&mut self, foliage_guard: &MutexGuard<Vec<Leaf>>) -> f64 {
        // Accumulate sum over inner products
        self.products
            .iter_mut()
            .map(|mul| mul.value(&foliage_guard))
            .reduce(|acc, v| acc + v)
            .unwrap_or_else(|| 0.0)
    }

    #[inline(always)]
    pub fn counted_value(&mut self, foliage_guard: &MutexGuard<Vec<Leaf>>) -> (f64, usize) {
        // Accumulate sum over inner products
        let (value, mut count) = self.products
            .iter_mut()
            .map(|mul| mul.counted_value(&foliage_guard))
            .reduce(|acc, (value, count)| 
                (acc.0 + value, acc.1 + count)
            )
            .unwrap_or_else(|| (0.0, 0));

        if self.products.len() >= 2 {
            count += self.products.len() - 1;
        }

        (value, count)
    }

    pub fn is_flat(&self) -> bool {
        !self.products.iter().any(|mul| !mul.is_flat())
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

    pub fn update_dependencies(&self, foliage_guard: &mut MutexGuard<Vec<Leaf>>) {
        for mul in &self.products {
            mul.update_dependencies(foliage_guard);
        }
    }

    pub fn count_adds(&self) -> usize {
        if self.products.is_empty() {
            0
        } else {
            1 + self.products.iter().fold(0, |acc, mul| acc + mul.count_adds())
        }
    }

    pub fn count_muls(&self) -> usize {
        self.products.len() + self.products.iter().fold(0, |acc, mul| acc + mul.count_muls())
    }

    pub fn layers(&self) -> usize {
        self.products.iter().map(|mul| mul.layers()).max().unwrap()
    }
 
    pub fn get_dot_text(
        &self,
        index: Option<u16>,
        foliage_guard: &MutexGuard<Vec<Leaf>>,
    ) -> (String, u16) {
        let mut dot_text = String::new();
        let index = if index.is_some() { index.unwrap() } else { 0 };

        let scope = &self.scope;
        dot_text += &format!("s_{index} [label=\"+\n{scope:?}\"]\n");

        let mut last = index;
        let mut sub_text;
        let mut next = index + 1;
        for mul in &self.products {
            dot_text += &format!("s_{index} -> p_{next}\n");
            (sub_text, last) = mul.get_dot_text(Some(next), &foliage_guard);
            dot_text += &sub_text;
            next = last + 1;
        }

        (dot_text, last)
    }

    // =============================== //
    // ===========  WRITE  =========== //
    pub fn add_mul(&mut self, mul: Mul) {
        self.scope.extend(&mul.scope);
        if mul.memory.is_some() {
            for own in &mut self.products {
                if own.factors == mul.factors && own.memory.is_some() {
                    own.memory = Memory::combine(&own.memory, &mul.memory);
                    own.scope.extend(mul.scope);
                    return;
                }
            }                
        }

        self.products.push(mul);
    }

    pub fn mul_index(&mut self, index: u16) {
        self.scope.insert(index);
        self.products
            .iter_mut()
            .for_each(|mul| mul.mul_index(index));
    }

    // pub fn remove(&mut self, index: usize) {
    //     if self.scope.remove(&index) {
    //         self.products.iter_mut().for_each(|mul| mul.remove(index));
    //     }
    // }

    pub fn collect(&mut self, index: u16, repeat: usize) -> Option<Collection> {
        // if !self.scope.contains(&index) {
        //     return None;
        // }

        let mut forwards = vec![];
        let mut applies = vec![];
        let mut to_remove = vec![];

        let active = self.products.iter().any(|mul| mul.factors.contains(&index));
        for i in 0..self.products.len() {
            match self.products[i].collect(index, active, repeat) {
                Some(Collection::Apply(muls)) => {
                    applies.push((muls, self.products[i].factors.clone()));
                    to_remove.push(i);
                }
                Some(Collection::Forward(mut muls)) => {
                    forwards.append(&mut muls);
                    to_remove.push(i);
                }
                None => continue,
            }
        }

        // Sanity check that leafs are all on single layer
        debug_assert!(applies.is_empty() || forwards.is_empty());

        // Remove products that have generated a Forward
        to_remove.reverse();
        to_remove.iter().for_each(|i| {
            self.products.remove(*i);
        });

        // Pass gathered collections up
        if !applies.is_empty() {
            self._apply_collection(index, applies);
            if repeat > 0 {
                self.collect(index, repeat - 1)
            } else {
                None
            }
        } else if !forwards.is_empty() {
            self.scope.remove(&index);
            Some(Collection::Forward(forwards))
        } else {
            None
        }
    }

    pub fn add_marked(&mut self, marked_mul: MarkedMul, index: u16) {
        match marked_mul {
            MarkedMul::Singleton => self.add_mul(Mul::new(vec![index])),
            MarkedMul::InScope(mul) => {
                let mut outer_mul = Mul::new(vec![index]);
                outer_mul.mul_add(Add::from_mul(mul));
                self.add_mul(outer_mul);
            }
            MarkedMul::OutOfScope(mul) => {
                let mut outer_mul = Mul::new(vec![]);
                outer_mul.mul_add(Add::from_mul(mul));
                self.add_mul(outer_mul);
            }
        }
    }

    pub fn _apply_collection(
        &mut self,
        index: u16,
        applies: Vec<(Vec<MarkedMul>, BTreeSet<u16>)>,
    ) {
        for (marked_muls, prefix) in applies {
            for marked_mul in marked_muls {
                self.add_marked(marked_mul, index);
            }

            let last = self.products.len() - 1;
            for i in &prefix {
                self.products[last].mul_index(*i);
            }
        }
    }

    pub fn disperse(&mut self, index: u16, repeat: usize, value: f64) {
        self.products
            .par_iter_mut()
            .filter(|mul| mul.scope.contains(&index))
            .for_each(|mul| mul.disperse(index, repeat, value));

        self.products.shrink_to_fit();
    }

    pub fn deploy(&self) -> Vec<Memory> {
        let mut memories = vec![];
        for mul in &self.products {
            memories.append(&mut mul.deploy());
        }

        memories
    }

    pub fn empty_scope(&mut self) {
        self.scope.clear();
        self.products.iter_mut().for_each(|mul| mul.empty_scope());
    }
}

impl ops::Mul<u16> for Add {
    type Output = Mul;

    fn mul(self, index: u16) -> Mul {
        let mut mul = Mul::empty_new();

        mul.mul_add(self);
        mul.mul_index(index);

        mul
    }
}

impl ops::Add<Add> for Add {
    type Output = Add;

    fn add(self, other: Add) -> Add {
        let mut add = Add::empty_new();

        self.products
            .iter()
            .for_each(|mul| add.add_mul(mul.clone()));
        other
            .products
            .iter()
            .for_each(|mul| add.add_mul(mul.clone()));

        add
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::circuit::rc::RC;

    #[test]
    fn test_add() {
        // Create basic RC
        let mut rc = RC::new();
        rc.grow(0.5, "a");
        rc.grow(0.5, "b");

        // Empty adder should return 0
        let mut add = Add::empty_new();
        assert_eq!(add.value(&rc.foliage.lock().unwrap()), 0.0);

        // Add over single mul should return result of mul
        let mut mul = Mul::new(vec![0, 1]);
        add.add_mul(mul.clone());
        let mul_value = mul.value(&rc.foliage.lock().unwrap());
        let add_value = add.value(&rc.foliage.lock().unwrap());
        assert_eq!(mul_value, add_value);

        // Scope of add needs to be all leafs and sorted
        assert_eq!(add.scope, BTreeSet::from_iter(vec![0, 1]));
    }
}
