use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Acquire;
use std::sync::atomic::Ordering::Release;
use std::sync::{Arc, MutexGuard};

use atomic_float::AtomicF64;

use super::add::Add;
use super::leaf::Leaf;
use super::mul::Collection;
use super::mul::Mul;

#[derive(Clone)]
pub struct Memory {
    pub storage: Arc<AtomicF64>,
    pub valid: Arc<AtomicBool>,
    pub add: Add,
}

impl Memory {
    // ============================= //
    // ========  CONSTRUCT  ======== //
    pub fn new(storage: f64, valid: bool, add: Option<Add>) -> Self {
        let add = if add.is_some() {
            add.unwrap()
        } else {
            Add::empty_new()
        };

        Self {
            storage: Arc::new(AtomicF64::new(storage)),
            valid: Arc::new(AtomicBool::new(valid)),
            add,
        }
    }

    // ============================== //
    // ===========  READ  =========== //
    pub fn is_flat(&self) -> bool {
        self.add.is_flat()
    }

    pub fn is_equal(&self, other: &Memory) -> bool {
        self.add.is_equal(&other.add)
    }

    pub fn update_dependencies(&self, foliage_guard: &mut MutexGuard<Vec<Leaf>>) {
        self.add.update_dependencies(foliage_guard);
    }

    pub fn get_dot_text(
        &self,
        index: Option<usize>,
        foliage_guard: &MutexGuard<Vec<Leaf>>,
    ) -> (String, usize) {
        let mut dot_text = String::new();
        let index = if index.is_some() { index.unwrap() } else { 0 };

        dot_text += &format!(
            "m_{index} [shape=rect, label=\"Memory\n{value:.2} | {valid}\"]\n",
            index = index,
            value = self.storage.load(Acquire),
            valid = self.valid.load(Acquire)
        );

        let sub_text;
        let mut last = index;
        if !self.add.products.is_empty() {
            let next = index + 1;
            dot_text += &format!("m_{index} -> s_{next}\n");
            (sub_text, last) = self.add.get_dot_text(Some(next), foliage_guard);
            dot_text += &sub_text;
        }

        (dot_text, last)
    }

    // =============================== //
    // ===========  WRITE  =========== //
    pub fn value(&mut self, foliage_guard: &MutexGuard<Vec<Leaf>>) -> f64 {
        match self.valid.load(Acquire) {
            true => self.storage.load(Acquire),
            false => {
                self.storage.store(self.add.value(&foliage_guard), Release);
                self.valid.store(true, Release);

                self.storage.load(Acquire)
            }
        }
    }

    pub fn counted_value(&mut self, foliage_guard: &MutexGuard<Vec<Leaf>>) -> (f64, usize) {
        match self.valid.load(Acquire) {
            true => (self.storage.load(Acquire), 0),
            false => {
                let (value, operations_count) = self.add.counted_value(&foliage_guard);
                self.storage.store(value, Release);
                self.valid.store(true, Release);

                (self.storage.load(Acquire), operations_count)
            }
        }
    }

    pub fn invalidate(&mut self) {
        self.valid.store(false, Release);
    }

    // pub fn remove(&mut self, index: usize) {
    //     self.valid = false;

    //     match &mut self.add {
    //         Some(add) => add.remove(index),
    //         None => (),
    //     }
    // }

    pub fn add(&mut self, mul: Mul) {
        self.valid.store(false, Release);
        self.add.add_mul(mul);
    }

    pub fn mul_index(&mut self, index: usize) {
        self.valid.store(false, Release);
        self.add.mul_index(index);
    }

    pub fn collect(&mut self, index: usize) -> Option<Collection> {
        match self.add.collect(index) {
            Some(Collection::Apply(_)) => {
                panic!("MemoryCells should never get Collection::Apply!")
            }
            Some(Collection::Forward(muls)) => {
                if self.add.products.is_empty() {
                    self.add = Add::empty_new();
                }
                self.valid.store(false, Release);
                Some(Collection::Apply(muls))
            }
            None => None,
        }
    }

    pub fn disperse(&mut self, index: usize) {
        self.add.disperse(index);
    }
}
