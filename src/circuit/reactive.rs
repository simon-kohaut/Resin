// Standard library
use std::ops::{Add, Mul};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, MutexGuard};
use std::str::FromStr;
use std::fs::File;
use std::io::Write;
use std::process::Command;

// Third-party
use atomic_float::AtomicF64;
use rayon::prelude::{IntoParallelRefMutIterator, ParallelIterator};

// Crate
use crate::Leaf;

#[derive(Clone)]
pub struct ReactiveCircuit {
    pub storage: Arc<AtomicF64>,
    pub valid: Arc<AtomicBool>,
    pub products: Vec<(Vec<u16>, Option<ReactiveCircuit>)>,
}

impl ReactiveCircuit {
    pub fn new() -> Self {
        Self {
            storage: Arc::new(AtomicF64::new(0.0)),
            valid: Arc::new(AtomicBool::new(false)),
            products: vec![],
        }
    }

    pub fn value(&mut self, foliage: &MutexGuard<Vec<Leaf>>) -> f64 {
        if self.valid.load(Ordering::Acquire) {
            return self.storage.load(Ordering::Acquire);
        }

        let value = self
            .products
            .iter_mut()
            .map(|(factors, sub_rc)| {
                let value = if sub_rc.is_some() {
                    sub_rc.as_mut().unwrap().value(&foliage)
                } else {
                    1.0
                };

                factors
                    .iter()
                    .map(|index| foliage[*index as usize].get_value())
                    .product::<f64>()
                    * value
            })
            .reduce(|acc, v| acc + v)
            .unwrap_or(0.0);

        self.valid.store(true, Ordering::Release);
        self.storage.store(value, Ordering::Release);

        value
    }

    pub fn counted_value(&mut self, foliage: &MutexGuard<Vec<Leaf>>) -> (f64, usize) {
        if self.valid.load(Ordering::Acquire) {
            return (self.storage.load(Ordering::Acquire), 0);
        }

        let (value, count) =
            self.products
                .iter_mut()
                .fold((1.0, 0), |(acc_value, acc_count), (factors, sub_rc)| {
                    // Get product of leafs
                    let product_value = factors.iter().fold(1.0, |acc, factor| {
                        acc * foliage[*factor as usize].get_value()
                    });

                    // Factor in the optional result of ReactiveCircuit underneath
                    let (inner_value, inner_count) = if sub_rc.is_some() {
                        sub_rc.as_mut().unwrap().counted_value(&foliage)
                    } else {
                        (1.0, 0)
                    };

                    // Add another 1 to count since we sum up two value
                    (
                        acc_value + product_value * inner_value,
                        acc_count + factors.len() + inner_count + 1,
                    )
                });

        self.valid.store(true, Ordering::Release);
        self.storage.store(value, Ordering::Release);

        (value, count)
    }

    pub fn clear_dependencies(&self, foliage: &mut MutexGuard<Vec<Leaf>>) {
        for leaf in foliage.iter_mut() {
            leaf.remove_dependency(&self.valid);
        }

        self.products.iter().for_each(|(_, sub_rc)| {
            if sub_rc.is_some() {
                sub_rc.as_ref().unwrap().clear_dependencies(foliage)
            }
        });
    }

    pub fn set_dependencies(&self, foliage: &mut MutexGuard<Vec<Leaf>>) {
        for leaf in 0..foliage.len() {
            self.products.iter().for_each(|(factors, _)| {
                if factors.contains(&(leaf as u16)) {
                    foliage[leaf].add_dependency(self.valid.clone());
                }
            });
        }

        self.products.iter().for_each(|(_, sub_rc)| {
            if sub_rc.is_some() {
                sub_rc.as_ref().unwrap().set_dependencies(foliage)
            }
        });
    }

    pub fn drop(&mut self, leaf: u16) {
        self.products.par_iter_mut().for_each(|(factors, sub_rc)| {
            match factors.iter().position(|index| *index == leaf) {
                Some(position) => {
                    if sub_rc.is_some() {
                        *sub_rc = Some(sub_rc.clone().unwrap() * leaf);
                    } else {
                        *sub_rc = Some(ReactiveCircuit::new() + leaf);
                    }

                    factors.remove(position);
                }
                None => {
                    if sub_rc.is_some() {
                        sub_rc.as_mut().unwrap().drop(leaf);
                    }
                }
            }
        });
    }

    pub fn deploy(&self) -> Vec<ReactiveCircuit> {
        let mut rcs = vec![self.clone()];
        self.products.iter().for_each(|(_, sub_rc)| {
            if sub_rc.is_some() {
                rcs.append(&mut sub_rc.as_ref().unwrap().deploy());
            };
        });

        rcs
    }

    pub fn to_svg(&self, path: &str, foliage: &MutexGuard<Vec<Leaf>>) -> std::io::Result<()> {
        let mut dot_text = String::from_str("strict digraph {\nnode [shape=circle]\n").unwrap();
        dot_text += &self.get_dot_text(Some(0), foliage).0;
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
    
    pub fn get_dot_text(
        &self,
        index: Option<u16>,
        foliage: &MutexGuard<Vec<Leaf>>,
    ) -> (String, u16) {
        let mut dot_text = String::new();
        let index = if index.is_some() { index.unwrap() } else { 0 };

        let mut last = index;
        for (factors, sub_circuit) in &self.products {
            for factor in factors {
                let name = foliage[*factor as usize].name.to_owned();
                dot_text += &format!("rc_{index} -> {name}\n");
            }

            let sub_text;
            if sub_circuit.is_some() {
                let sub_circuit = sub_circuit.as_ref().unwrap();
                let next = index + 1;
    
                dot_text += &format!("rc_{index} -> rc_{next}\n");
                (sub_text, last) = sub_circuit.get_dot_text(Some(next), foliage);
                dot_text += &sub_text;
            }
        }

        (dot_text, last)
    }
}

impl Add<ReactiveCircuit> for ReactiveCircuit {
    type Output = ReactiveCircuit;

    fn add(mut self, rhs: ReactiveCircuit) -> Self::Output {
        // Store the sum of both memorized values
        self.storage.store(
            self.storage.load(Ordering::Acquire) + rhs.storage.load(Ordering::Acquire),
            Ordering::Release,
        );

        // If both where valid, this keeps valid
        self.valid.store(
            self.valid.load(Ordering::Acquire) && rhs.valid.load(Ordering::Acquire),
            Ordering::Release,
        );

        // Add up the products of rhs
        for (factors, sub_rc) in &rhs.products {
            self.products.push((factors.clone(), sub_rc.clone()));
        }

        self
    }
}

impl Add<u16> for ReactiveCircuit {
    type Output = ReactiveCircuit;

    fn add(mut self, rhs: u16) -> Self::Output {
        self.products.push((vec![rhs], None));

        self
    }
}

impl Add<Vec<u16>> for ReactiveCircuit {
    type Output = ReactiveCircuit;

    fn add(mut self, rhs: Vec<u16>) -> Self::Output {
        self.products.push((rhs, None));

        self
    }
}

impl Mul<u16> for ReactiveCircuit {
    type Output = ReactiveCircuit;

    fn mul(mut self, rhs: u16) -> Self::Output {
        // We do not have the leaf value, only its index, so cannot stay valid
        self.valid.store(false, Ordering::Release);

        // Combine own products with new leaf index
        for (factors, _) in &mut self.products {
            factors.push(rhs);
        }

        self
    }
}


#[cfg(test)]
mod tests {

    use crate::circuit::leaf::update;
    use super::*;

    use std::sync::{Arc, Mutex};

    #[test]
    fn test_rc() {
        let mut rc = ReactiveCircuit::new();
        let foliage = Arc::new(Mutex::new(vec![
            Leaf::new(&0.5, &0.0, "0"),
            Leaf::new(&0.75, &0.0, "1"),
            Leaf::new(&0.25, &0.0, "2"),
            Leaf::new(&0.1, &0.0, "3")
        ]));

        rc = rc + 0 + vec![1 as u16, 2  as u16, 3  as u16]; 
        assert_eq!(rc.value(&foliage.lock().unwrap()), 0.5 + 0.75 * 0.25 * 0.1);

        rc.drop(2);
        assert_eq!(rc.value(&foliage.lock().unwrap()), 0.5 + 0.75 * 0.25 * 0.1);

        rc.set_dependencies(&mut foliage.lock().unwrap());
        update(foliage.clone(), 1, &0.1);
        update(foliage.clone(), 2, &0.3);
        assert!(!rc.valid.load(Ordering::Acquire));
        assert_eq!(rc.value(&foliage.lock().unwrap()), 0.5 + 0.1 * 0.3 * 0.1);
        assert!(rc.valid.load(Ordering::Acquire));

        rc.clear_dependencies(&mut foliage.lock().unwrap());
        update(foliage.clone(), 1, &0.1);
        assert!(rc.valid.load(Ordering::Acquire));

        let deployed = rc.deploy();
        assert_eq!(deployed[0].valid.as_ptr(), rc.valid.as_ptr());
    }
}