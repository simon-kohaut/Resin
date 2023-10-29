use std::collections::BTreeSet;
use std::ops;
use std::sync::MutexGuard;

use super::leaf::Leaf;
use super::mul::Collection;
use super::mul::MarkedMul;
use super::mul::Mul;

#[derive(Clone)]
pub struct Add {
    pub scope: BTreeSet<usize>,
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

    pub fn from_index_matrix(index_matrix: Vec<Vec<usize>>) -> Self {
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
    pub fn value(&mut self, foliage_guard: &MutexGuard<Vec<Leaf>>) -> f64 {
        // Accumulate sum over inner products
        self.products
            .iter_mut()
            .fold(0.0, |acc, mul| acc + mul.value(&foliage_guard))
    }

    pub fn counted_value(&mut self, foliage_guard: &MutexGuard<Vec<Leaf>>) -> (f64, usize) {
        // Accumulate sum over inner products
        let (value, mut count) = self.products.iter_mut().fold((0.0, 0), |mut acc, mul| {
            let (value, operations_count) = mul.counted_value(&foliage_guard);
            acc.0 += value;
            acc.1 += operations_count;
            acc
        });

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

    pub fn get_dot_text(
        &self,
        index: Option<usize>,
        foliage_guard: &MutexGuard<Vec<Leaf>>,
    ) -> (String, usize) {
        let mut dot_text = String::new();
        let index = if index.is_some() { index.unwrap() } else { 0 };

        dot_text += &format!("s_{index} [label=\"+\"]\n");

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
        self.products.push(mul);
    }

    pub fn mul_index(&mut self, index: usize) {
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

    pub fn collect(&mut self, index: usize) -> Option<Collection> {
        let mut forwards = vec![];
        let mut applies = vec![];
        let mut to_remove = vec![];

        if !self.scope.contains(&index) {
            return None;
        }

        let active = self.products.iter().any(|mul| mul.factors.contains(&index));
        for i in 0..self.products.len() {
            match self.products[i].collect(index, active) {
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
            None
        } else if !forwards.is_empty() {
            self.scope.remove(&index);
            Some(Collection::Forward(forwards))
        } else {
            None
        }
    }

    pub fn add_marked(&mut self, marked_mul: MarkedMul, index: usize) {
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
        index: usize,
        applies: Vec<(Vec<MarkedMul>, BTreeSet<usize>)>,
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

    pub fn disperse(&mut self, index: usize) {
        self.products
            .iter_mut()
            .filter(|mul| mul.scope.contains(&index))
            .for_each(|mul| mul.disperse(index));
    }
}

impl ops::Mul<usize> for Add {
    type Output = Mul;

    fn mul(self, index: usize) -> Mul {
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
