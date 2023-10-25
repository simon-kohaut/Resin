use super::add::Add;
use super::leaf::Foliage;
use super::mul::Collection;
use super::mul::Mul;

pub struct Memory {
    pub storage: f64,
    pub valid: bool,
    pub add: Option<Add>,
    pub foliage: Foliage,
}

impl Memory {
    // ============================= //
    // ========  CONSTRUCT  ======== //
    pub fn new(storage: f64, valid: bool, add: Option<Add>, foliage: Foliage) -> Self {
        Self {
            storage,
            valid,
            add,
            foliage,
        }
    }

    pub fn new_one(foliage: Foliage) -> Self {
        Self {
            storage: 1.0,
            valid: true,
            add: None,
            foliage,
        }
    }

    // ============================== //
    // ===========  READ  =========== //
    pub fn is_flat(&self) -> bool {
        match &self.add {
            Some(add) => add.is_flat(),
            None => true,
        }
    }

    pub fn is_equal(&self, other: &Memory) -> bool {
        if self.add.is_none() && other.add.is_none() {
            true
        } else {
            self.add
                .as_ref()
                .unwrap()
                .is_equal(other.add.as_ref().unwrap())
        }
    }

    pub fn get_dot_text(&self, index: Option<usize>) -> (String, usize) {
        let mut dot_text = String::new();
        let index = if index.is_some() { index.unwrap() } else { 0 };

        dot_text += &format!(
            "m_{index} [shape=rect, label=\"Memory\n{value:.2} | {valid}\"]\n",
            index = index,
            value = self.storage,
            valid = self.valid
        );

        let sub_text;
        let mut last = index;
        match &self.add {
            Some(add) => {
                let next = index + 1;
                dot_text += &format!("m_{index} -> s_{next}\n");
                (sub_text, last) = add.get_dot_text(Some(next));
                dot_text += &sub_text;
            }
            None => (),
        }

        (dot_text, last)
    }

    // =============================== //
    // ===========  WRITE  =========== //
    pub fn value(&mut self) -> f64 {
        match self.valid {
            true => self.storage,
            false => {
                self.storage = if self.add.is_some() {
                    self.add.as_ref().unwrap().value()
                } else {
                    1.0
                };
                self.valid = true;

                self.storage
            }
        }
    }

    pub fn invalidate(&mut self) {
        self.valid = false;
    }

    pub fn remove(&mut self, index: usize) {
        self.valid = false;

        match &mut self.add {
            Some(add) => add.remove(index),
            None => (),
        }
    }

    pub fn add(&mut self, mul: Mul) {
        self.valid = false;

        match &mut self.add {
            Some(add) => add.add_mul(mul),
            None => {
                self.add = Some(Add::empty_new(self.foliage.clone()));
                self.add.as_mut().unwrap().add_mul(mul);
            }
        }
    }

    pub fn mul_index(&mut self, index: usize) {
        self.valid = false;

        match &mut self.add {
            Some(add) => add.mul_index(index),
            None => {
                self.add = Some(Add::empty_new(self.foliage.clone()));
                self.add.as_mut().unwrap().mul_index(index);
            }
        }
    }

    pub fn collect(&mut self, index: usize) -> Option<Collection> {
        match &mut self.add {
            Some(add) => match add.collect(index) {
                Some(Collection::Apply(_)) => {
                    panic!("MemoryCells should never get Collection::Apply!")
                }
                Some(Collection::Forward(muls)) => {
                    if add.products.is_empty() {
                        self.add = None;
                    }
                    self.valid = false;
                    Some(Collection::Apply(muls))
                }
                None => None,
            },
            None => None,
        }
    }

    pub fn disperse(&mut self, index: usize) {
        match &mut self.add {
            Some(add) => {
                add.disperse(index);

                // Check if this layer is no longer useful
                if add.products.iter().all(|mul| mul.factors.is_empty()) {
                    let mut merged_add = Add::empty_new(self.foliage.clone());
                    for mul in &add.products {
                        match &mul.memory {
                            Some(memory) => match &memory.lock().unwrap().add {
                                Some(inner_add) => inner_add
                                    .products
                                    .iter()
                                    .for_each(|mul| merged_add.add_mul(mul.clone())),
                                None => (),
                            },
                            None => (),
                        }
                    }
                    self.add = Some(merged_add);
                }
            }
            None => (),
        }
    }
}
