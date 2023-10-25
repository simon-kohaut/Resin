use chashmap::CHashMap;
use core::panic;
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
    pub memory: Arc<Mutex<Memory>>,
    pub foliage: Foliage,
}

impl RC {
    // ============================= //
    // ========  CONSTRUCT  ======== //
    pub fn new() -> Self {
        let foliage = Arc::new(Mutex::new(vec![]));
        let memory = Arc::new(Mutex::new(Memory::new(
            0.0,
            true,
            Some(Add::empty_new(foliage.clone())),
            foliage.clone(),
        )));

        Self { memory, foliage }
    }

    pub fn get_dot_text(&self) -> String {
        let (dot_text, _) = self.memory.lock().unwrap().get_dot_text(Some(0));
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
            .args(["-Tsvg", &path])
            .output()
            .expect("Failed to run graphviz!");

        file = File::create(path)?;
        file.write_all(&svg_text.stdout)?;
        file.sync_all()?;

        Ok(())
    }

    // ============================== //
    // ===========  READ  =========== //
    pub fn value(&self) -> f64 {
        self.memory.lock().unwrap().value()
    }

    pub fn is_flat(&self) -> bool {
        self.memory.lock().unwrap().is_flat()
    }

    // =============================== //
    // ===========  WRITE  =========== //
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
        let mut memory_guard = self.memory.lock().unwrap();
        memory_guard.add(mul);
    }

    pub fn remove(&mut self, index: usize) {
        let mut memory_guard = self.memory.lock().unwrap();
        memory_guard.remove(index);
    }

    pub fn collect(&mut self, index: usize) {
        let mut memory_guard = self.memory.lock().unwrap();
        match memory_guard.collect(index) {
            Some(Collection::Apply(collection)) => {
                let mut add = Add::empty_new(self.foliage.clone());
                add._apply_collection(index, vec![(collection, vec![])]);
                memory_guard.add = Some(add);
            }
            Some(Collection::Forward(_)) => panic!("RC got Forward collection!"),
            None => (),
        }
    }

    pub fn disperse(&mut self, index: usize) {
        let mut memory_guard = self.memory.lock().unwrap();
        memory_guard.disperse(index);
    }
}

#[cfg(test)]
mod tests {

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
        let mul = Mul::new(vec![0, 1], rc.foliage.clone());
        rc.add(mul.clone());
        assert_eq!(mul.value(), rc.value());

        // The root add should have both leafs in scope
        assert_eq!(
            rc.memory.lock().unwrap().add.as_ref().unwrap().scope,
            vec![0, 1]
        );

        // Dispersing should not change the value
        let value_before = rc.value();
        rc.disperse(0);
        assert_eq!(value_before, rc.value());

        // The root add should still have both leafs in scope
        assert_eq!(
            rc.memory.lock().unwrap().add.as_ref().unwrap().scope,
            vec![0, 1]
        );

        // Collecting should not change the value
        rc.collect(0);
        assert_eq!(value_before, rc.value());

        // The root add should still have both leafs in scope
        assert_eq!(
            rc.memory.lock().unwrap().add.as_ref().unwrap().scope,
            vec![0, 1]
        );

        // We should be able to remove and thereby (potentially) divide the value
        rc.remove(0);
        assert_eq!(rc.value(), 0.5);

        // The root add should no longer have both leafs in scope
        assert_eq!(
            rc.memory.lock().unwrap().add.as_ref().unwrap().scope,
            vec![1]
        );
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
        rc.add(Mul::new(vec![0, 1], rc.foliage.clone()));
        rc.add(Mul::new(vec![1], rc.foliage.clone()));
        rc.add(Mul::new(vec![0, 2], rc.foliage.clone()));
        rc.add(Mul::new(vec![1, 3], rc.foliage.clone()));

        // This RC should be considered flat
        rc.to_svg("output/flat_test_original.svg");
        assert!(rc.is_flat());

        // It should no longer be flat after collect/disperse are applied
        rc.collect(1);
        rc.to_svg("output/flat_test_collect_1.svg");
        assert!(!rc.is_flat());

        // Disperse after collect for the same leaf should make it flat again
        rc.disperse(1);
        rc.to_svg("output/flat_test_collect_1_disperse_1.svg");
        assert!(rc.is_flat());

        // Any balanced combination of collect and disperse should get us back to a flat RC
        rc.collect(2);
        rc.to_svg("output/flat_test_collect_2.svg");
        rc.disperse(3);
        rc.to_svg("output/flat_test_collect_2_disperse_3.svg");
        rc.collect(1);
        rc.collect(3);
        rc.disperse(1);
        rc.disperse(2);
        assert!(rc.is_flat());
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
        rc.add(Mul::new(vec![0, 1], rc.foliage.clone()));
        rc.add(Mul::new(vec![1], rc.foliage.clone()));
        rc.add(Mul::new(vec![0, 2], rc.foliage.clone()));
        rc.add(Mul::new(vec![1, 3], rc.foliage.clone()));

        // This RC should be considered flat
        rc.to_svg("output/disperse_original.svg");
        assert!(rc.is_flat());

        // It should no longer be flat after collect/disperse are applied
        rc.disperse(1);
        rc.to_svg("output/disperse_1.svg");
        assert!(!rc.is_flat());

        // Disperse after collect for the same leaf should make it flat again
        rc.disperse(1);
        rc.to_svg("output/disperse_1_1.svg");
    }

    #[test]
    fn test_dot_text() -> std::io::Result<()> {
        // Create basic RC
        let mut rc = RC::new();
        rc.grow(0.5, "a");
        rc.grow(0.5, "b");

        // Test for a simple multiplication (rc = a * b)
        rc.add(Mul::new(vec![0, 1], rc.foliage.clone()));
        rc.to_svg("output/test_rc_svg.svg")?;
        let mut expected_text = "\
            m_0 [shape=rect, label=\"Memory\n0.00 | false\"]\n\
            m_0 -> s_1\n\
            s_1 [label=\"+\"]\n\
            s_1 -> p_2\n\
            p_2 [label=\"&times;\"]\n\
            p_2 -> a\n\
            p_2 -> b\n\
        ";
        assert_eq!(rc.get_dot_text(), expected_text);

        rc.disperse(1);
        rc.to_svg("output/test_rc_svg_drop_b.svg")?;
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
        assert_eq!(rc.get_dot_text(), expected_text);

        rc.collect(1);
        rc.to_svg("output/test_rc_svg_drop_b_collect_b.svg")?;
        expected_text = "\
            m_0 [shape=rect, label=\"Memory\n0.00 | false\"]\n\
            m_0 -> s_1\n\
            s_1 [label=\"+\"]\n\
            s_1 -> p_2\n\
            p_2 [label=\"&times;\"]\n\
            p_2 -> a\n\
            p_2 -> b\n\
        ";
        assert_eq!(rc.get_dot_text(), expected_text);

        Ok(())
    }
}
