use core::panic;
use std::collections::BTreeSet;
use std::fs::File;
use std::io::Write;
use std::process::Command;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use super::add::Add;
use super::leaf::Foliage;
use super::memory::Memory;
use super::mul::{Collection, Mul};
use super::Leaf;

pub struct RC {
    pub memory: Memory,
    pub foliage: Foliage,
}

impl RC {
    // ============================= //
    // ========  CONSTRUCT  ======== //
    pub fn new() -> Self {
        let foliage = Arc::new(Mutex::new(vec![]));
        let memory = Memory::new(0.0, true, Some(Add::empty_new()));

        Self { memory, foliage }
    }

    // ============================== //
    // ===========  READ  =========== //
    pub fn is_flat(&self) -> bool {
        self.memory.is_flat()
    }

    pub fn clear_dependencies(&self) {
        let mut foliage_guard = self.foliage.lock().unwrap();
        for leaf in foliage_guard.iter_mut() {
            leaf.clear_dependencies();
        }
    }

    pub fn update_dependencies(&self) {
        self.clear_dependencies();

        let mut foliage_guard = self.foliage.lock().unwrap();
        for index in &self.memory.add.scope {
            foliage_guard[*index].add_dependency(self.memory.valid.clone());
        }

        self.memory.update_dependencies(&mut foliage_guard);
    }

    pub fn get_dot_text(&self) -> String {
        let foliage_guard = self.foliage.lock().unwrap();
        let (dot_text, _) = self.memory.get_dot_text(Some(0), &foliage_guard);
        dot_text
    }

    pub fn to_svg(&self, path: &str) -> std::io::Result<()> {
        let mut dot_text = String::from_str("strict digraph {\nnode [shape=circle]\n").unwrap();
        dot_text += &self.get_dot_text();
        dot_text += "}";

        let mut file = File::create(path)?;
        file.write_all(dot_text.as_bytes())?;
        file.sync_all()?;

        let svg_text = Command::new("dot")
            .args(["-Tsvg", path])
            .output()
            .expect("Failed to run graphviz!");

        file = File::create(path)?;
        file.write_all(&svg_text.stdout)?;
        file.sync_all()?;

        Ok(())
    }

    // =============================== //
    // ===========  WRITE  =========== //
    pub fn value(&mut self) -> f64 {
        let foliage_guard = self.foliage.lock().unwrap();
        self.memory.value(&foliage_guard)
    }

    pub fn counted_value(&mut self) -> (f64, usize) {
        let foliage_guard = self.foliage.lock().unwrap();
        self.memory.counted_value(&foliage_guard)
    }

    pub fn grow(&mut self, value: f64, name: &str) -> usize {
        self.foliage
            .lock()
            .unwrap()
            .push(Leaf::new(&value, &0.0, name));
        self.foliage.lock().unwrap().len() - 1
    }

    pub fn attach(&mut self, leaf: Leaf) {
        self.foliage.lock().unwrap().push(leaf);
    }

    pub fn add(&mut self, mul: Mul) {
        self.memory.add(mul);
    }

    // pub fn remove(&mut self, index: usize) {
    //     let mut memory_guard = self.memory.lock().unwrap();
    //     memory_guard.remove(index);
    // }

    pub fn collect(&mut self, index: usize) {
        match self.memory.collect(index) {
            Some(Collection::Apply(collection)) => {
                let mut add = Add::empty_new();
                add._apply_collection(index, vec![(collection, BTreeSet::new())]);
                self.memory.add = add;
            }
            Some(Collection::Forward(_)) => panic!("RC got Forward collection!"),
            None => (),
        }
    }

    pub fn disperse(&mut self, index: usize) {
        self.memory.disperse(index);

        // Check if this layer is no longer useful
        if self
            .memory
            .add
            .products
            .iter()
            .all(|mul| mul.factors.is_empty())
        {
            let mut merged_add = Add::empty_new();
            for mul in &self.memory.add.products {
                match &mul.memory {
                    Some(memory) => memory
                        .add
                        .products
                        .iter()
                        .for_each(|mul| merged_add.add_mul(mul.clone())),
                    None => (),
                }
            }

            self.memory.add = merged_add;
        }
    }
}

#[cfg(test)]
mod tests {

    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn test_rc() {
        // Create basic RC
        let mut rc = RC::new();
        rc.grow(0.5, "a");
        rc.grow(0.5, "b");

        // Empty RC should return 0
        assert_eq!(rc.value(), 0.0);

        // Mul should have value 0.5 * 0.5 = 0.25
        let mut mul = Mul::new(vec![0, 1]);
        rc.add(mul.clone());
        let mul_value = mul.value(&rc.foliage.lock().unwrap());
        assert_eq!(mul_value, rc.value());

        // The root add should have both leafs in scope
        assert_eq!(rc.memory.add.scope, BTreeSet::from_iter(vec![0, 1]));

        // Dispersing should not change the value
        let value_before = rc.value();
        rc.disperse(0);
        assert_eq!(value_before, rc.value());

        // The root add should still have both leafs in scope
        assert_eq!(rc.memory.add.scope, BTreeSet::from_iter(vec![0, 1]));

        // Collecting should not change the value
        rc.collect(0);
        assert_eq!(value_before, rc.value());

        // The root add should still have both leafs in scope
        assert_eq!(rc.memory.add.scope, BTreeSet::from_iter(vec![0, 1]));
    }

    #[test]
    fn test_collect_disperse() {
        // Create basic RC
        let mut rc = RC::new();
        rc.grow(0.5, "a");
        rc.grow(0.5, "b");
        rc.grow(0.5, "c");
        rc.grow(0.5, "d");

        // Build some combinations of the given leafs
        rc.add(Mul::new(vec![0, 1]));
        rc.add(Mul::new(vec![1]));
        rc.add(Mul::new(vec![0, 2]));
        rc.add(Mul::new(vec![1, 3]));

        // This RC should be considered flat
        assert!(rc.is_flat());

        // It should no longer be flat after collect/disperse are applied
        rc.collect(1);
        assert!(!rc.is_flat());

        // Disperse after collect for the same leaf should make it flat again
        rc.disperse(1);
        assert!(rc.is_flat());

        // Any balanced combination of collect and disperse should get us back to a flat RC
        rc.collect(2);
        rc.disperse(3);
        rc.collect(1);
        rc.collect(3);
        rc.disperse(1);
        rc.disperse(2);
        assert!(rc.is_flat());
    }

    #[test]
    fn test_collect() {
        // Create basic RC
        let mut rc = RC::new();
        rc.grow(0.5, "a");
        rc.grow(0.5, "b");
        rc.grow(0.5, "c");
        rc.grow(0.5, "d");

        // Build some combinations of the given leafs
        rc.add(Mul::new(vec![0, 1]));
        rc.add(Mul::new(vec![1]));
        rc.add(Mul::new(vec![0, 2]));
        rc.add(Mul::new(vec![1, 3]));

        // This RC should be considered flat
        assert!(rc.is_flat());

        // It should no longer be flat after collect/disperse are applied
        rc.collect(1);
        assert!(!rc.is_flat());

        // There should be 3 multiplications with that leaf in scope
        assert_eq!(
            rc.memory
                .add
                .products
                .iter()
                .filter(|mul| mul.scope.contains(&1))
                .collect::<Vec<_>>()
                .len(),
            3
        );
    }

    #[test]
    fn test_disperse() {
        // Create basic RC
        let mut rc = RC::new();
        rc.grow(0.5, "a");
        rc.grow(0.5, "b");
        rc.grow(0.5, "c");
        rc.grow(0.5, "d");

        // Build some combinations of the given leafs
        rc.add(Mul::new(vec![0, 1]));
        rc.add(Mul::new(vec![1]));
        rc.add(Mul::new(vec![0, 2]));
        rc.add(Mul::new(vec![1, 3]));

        // This RC should be considered flat
        assert!(rc.is_flat());

        // It should no longer be flat after collect/disperse are applied
        rc.disperse(1);
        assert!(!rc.is_flat());

        // There should be 3 multiplications with that leaf in scope
        assert_eq!(
            rc.memory
                .add
                .products
                .iter()
                .filter(|mul| mul.scope.contains(&1))
                .collect::<Vec<_>>()
                .len(),
            3
        );
    }

    #[test]
    fn test_dot_text() -> std::io::Result<()> {
        // Create basic RC
        let mut rc = RC::new();
        rc.grow(0.5, "a");
        rc.grow(0.5, "b");

        // Test for a simple multiplication (rc = a * b)
        rc.add(Mul::new(vec![0, 1]));
        let mut expected_text = "\
            m_0 [shape=rect, label=\"Memory\n0.00 | false\"]\n\
            m_0 -> s_1\n\
            s_1 [label=\"+\"]\n\
            s_1 -> p_2\n\
            p_2 [label=\"&times;\"]\n\
            p_2 -> a\n\
            p_2 -> b\n\
        ";
        rc.to_svg("output/dot_test_1.svg");
        assert_eq!(rc.get_dot_text(), expected_text);

        rc.disperse(1);
        expected_text = "\
            m_0 [shape=rect, label=\"Memory\n0.00 | false\"]\n\
            m_0 -> s_1\n\
            s_1 [label=\"+\"]\n\
            s_1 -> p_2\n\
            p_2 [label=\"&times;\"]\n\
            p_2 -> a\n\
            p_2 -> m_3\n\
            m_3 [shape=rect, label=\"Memory\n-1.00 | false\"]\n\
            m_3 -> s_4\n\
            s_4 [label=\"+\"]\n\
            s_4 -> p_5\n\
            p_5 [label=\"&times;\"]\n\
            p_5 -> b\n\
        ";
        rc.to_svg("output/dot_test_2.svg");
        assert_eq!(rc.get_dot_text(), expected_text);

        rc.collect(1);
        expected_text = "\
            m_0 [shape=rect, label=\"Memory\n0.00 | false\"]\n\
            m_0 -> s_1\n\
            s_1 [label=\"+\"]\n\
            s_1 -> p_2\n\
            p_2 [label=\"&times;\"]\n\
            p_2 -> a\n\
            p_2 -> b\n\
        ";
        rc.to_svg("output/dot_test_3.svg");
        assert_eq!(rc.get_dot_text(), expected_text);

        // Value should be unchanged
        assert_eq!(rc.value(), 0.25);

        Ok(())
    }
}
