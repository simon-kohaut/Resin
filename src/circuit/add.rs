use std::ops;

use super::leaf::Foliage;
use super::mul::Collection;
use super::mul::MarkedMul;
use super::mul::Mul;

#[derive(Clone)]
pub struct Add {
    pub scope: Vec<usize>,
    pub products: Vec<Mul>,
    pub foliage: Foliage,
}

impl Add {
    // ============================= //
    // ========  CONSTRUCT  ======== //
    pub fn new(scope: Vec<usize>, products: Vec<Mul>, foliage: Foliage) -> Self {
        Self {
            scope,
            products,
            foliage,
        }
    }

    pub fn empty_new(foliage: Foliage) -> Self {
        Self {
            scope: vec![],
            products: vec![],
            foliage,
        }
    }

    pub fn from_index_matrix(index_matrix: Vec<Vec<usize>>, foliage: Foliage) -> Self {
        // Fill new Add structure with products and scope
        let mut add = Add::empty_new(foliage.clone());
        for leaf_indices in index_matrix {
            add.scope.append(&mut leaf_indices.to_owned());
            add.products.push(Mul::new(leaf_indices, foliage.clone()));
        }

        // Correct scope to only contain the unique elements
        add.scope.sort();
        add.scope.dedup();

        add
    }

    pub fn from_mul(mul: Mul) -> Add {
        let mut add = Add::empty_new(mul.foliage.clone());
        add.add_mul(mul);
        add
    }

    // ============================== //
    // ===========  READ  =========== //
    pub fn value(&self) -> f64 {
        // Accumulate sum over inner products
        self.products.iter().fold(0.0, |acc, mul| acc + mul.value())
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

    pub fn get_dot_text(&self, index: Option<usize>) -> (String, usize) {
        let mut dot_text = String::new();
        let index = if index.is_some() { index.unwrap() } else { 0 };

        dot_text += &format!("s_{index} [label=\"+\"]\n");

        let mut last = index;
        let mut sub_text;
        let mut next = index + 1;
        for mul in &self.products {
            dot_text += &format!("s_{index} -> p_{next}\n");
            (sub_text, last) = mul.get_dot_text(Some(next));
            dot_text += &sub_text;
            next = last + 1;
        }

        (dot_text, last)
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

    pub fn mul_index(&mut self, index: usize) {
        match self.scope.binary_search(&index) {
            Ok(_) => (),
            Err(position) => self.scope.insert(position, index),
        }

        self.products
            .iter_mut()
            .for_each(|mul| mul.mul_index(index));
    }

    pub fn remove(&mut self, index: usize) {
        if self.scope.contains(&index) {
            self.scope.retain(|i| *i != index);
            let _ = self.products.iter_mut().for_each(|mul| mul.remove(index));
        }
    }

    pub fn collect(&mut self, index: usize) -> Option<Collection> {
        let mut forwards = vec![];
        let mut applies = vec![];
        let mut to_remove = vec![];

        let active = self.products.iter().any(|mul| mul.factors.contains(&index));
        for i in 0..self.products.len() {
            match self.products[i].collect(index, active) {
                Some(Collection::Apply(mut muls)) => {
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
        if !applies.is_empty() && !forwards.is_empty() {
            panic!("A leaf is distributed over multiple layers!");
        }

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
            Some(Collection::Forward(forwards))
        } else {
            None
        }
    }

    pub fn _apply_collection(&mut self, index: usize, applies: Vec<(Vec<MarkedMul>, Vec<usize>)>) {
        for (marked_muls, mut prefix) in applies {
            let mut in_scope_add = Add::empty_new(self.foliage.clone());
            let mut out_of_scope_add = Add::empty_new(self.foliage.clone());

            for marked_mul in marked_muls {
                match marked_mul {
                    MarkedMul::Singleton => {
                        let mut factors = vec![index];
                        factors.append(&mut prefix);
                        self.add_mul(Mul::new(factors, self.foliage.clone()));
                    }
                    MarkedMul::InScope(mul) => in_scope_add.add_mul(mul.clone()),
                    MarkedMul::OutOfScope(mul) => out_of_scope_add.add_mul(mul.clone()),
                }
            }

            if !in_scope_add.products.is_empty() {
                let mut factors = vec![index];
                factors.append(&mut prefix);
                let mut leaf_mul = Mul::new(factors, self.foliage.clone());
                leaf_mul.mul_add(in_scope_add);
                for i in &prefix {
                    leaf_mul.mul_index(*i);
                }
                self.add_mul(leaf_mul);
            }

            if !out_of_scope_add.products.is_empty() {
                let mut factors = vec![];
                factors.append(&mut prefix);
                let mut empty_mul = Mul::new(factors, self.foliage.clone());
                empty_mul.mul_add(out_of_scope_add);
                self.add_mul(empty_mul);
            }
        }
    }

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
        let mut mul = Mul::empty_new(self.foliage.clone());

        mul.mul_add(self.clone());
        mul.mul_index(index);

        mul
    }
}

impl ops::Add<Add> for Add {
    type Output = Add;

    fn add(self, other: Add) -> Add {
        let mut add = Add::empty_new(self.foliage.clone());

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
        let mut add = Add::empty_new(rc.foliage.clone());
        assert_eq!(add.value(), 0.0);

        // Add over single mul should return result of mul
        let mul = Mul::new(vec![0, 1], rc.foliage.clone());
        add.add_mul(mul.clone());
        assert_eq!(mul.value(), add.value());

        // Scope of add needs to be all leafs and sorted
        assert_eq!(add.scope, vec![0, 1]);
    }
}
