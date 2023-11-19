// Standard library
use std::collections::BTreeSet;
use std::fs::File;
use std::io::Write;
use std::ops::{Add, AddAssign, Mul};
use std::process::Command;
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard};

// Third-party
use rayon::prelude::{IntoParallelRefMutIterator, IntoParallelRefIterator, ParallelIterator};

// Crate
use crate::Leaf;

pub type RcQueue = Arc<Mutex<BTreeSet<usize>>>;
pub type SharedCircuit = Arc<Mutex<ReactiveCircuit>>;

#[derive(Clone)]
pub struct ReactiveCircuit {
    pub storage: f64,
    pub products: Vec<(Vec<u16>, Option<SharedCircuit>)>,
    pub index: usize,
}

impl ReactiveCircuit {
    pub fn new() -> Self {
        Self {
            storage: 0.0,
            products: vec![],
            index: 0,
        }
    }

    pub fn full_update(&mut self, foliage: &MutexGuard<Vec<Leaf>>) {
        let value = self
            .products
            .iter_mut()
            .map(|(factors, sub_rc)| {
                let value = if sub_rc.is_some() {
                    sub_rc
                        .as_mut()
                        .unwrap()
                        .lock()
                        .unwrap()
                        .full_update(&foliage);
                    sub_rc.as_mut().unwrap().lock().unwrap().value()
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

        self.storage = value;
    }

    pub fn counted_update(&mut self, foliage: &MutexGuard<Vec<Leaf>>) -> usize {
        let (value, count) =
            self.products
                .iter_mut()
                .fold((0.0, 0), |(acc_value, acc_count), (factors, sub_rc)| {
                    // Get product of leafs
                    let mut value = factors.iter().fold(1.0, |acc, factor| {
                        acc * foliage[*factor as usize].get_value()
                    });

                    // How many numbers where multiplied
                    let mut count = if factors.len() < 2 {
                        0
                    } else {
                        factors.len() - 1
                    };

                    // Factor in the optional result of ReactiveCircuit underneath
                    if sub_rc.is_some() {
                        value *= sub_rc.as_mut().unwrap().lock().unwrap().value();
                        count += 1;
                    }

                    (acc_value + value, acc_count + count)
                });

        self.storage = value;

        if self.products.len() < 2 {
            count
        } else {
            count + self.products.len() - 1
        }
    }

    pub fn update(&mut self, leaf_values: &[f64]) {
        let value = self
            .products
            .iter()
            .fold(0.0, |acc, (factors, sub_rc)| {
                let value;
                unsafe {
                    value = match sub_rc {
                        Some(rc) => rc.lock().unwrap_unchecked().value(),
                        None => 1.0
                    };    
                }
                
                acc + factors
                    .iter()
                    .fold(value, |acc, index| acc * leaf_values[*index as usize])
            });

        self.storage = value;
    }

    pub fn value(&self) -> f64 {
        self.storage
    }

    pub fn clear_dependencies(&self, foliage: &mut MutexGuard<Vec<Leaf>>) {
        for leaf in foliage.iter_mut() {
            leaf.remove_dependency(self.index);
        }

        self.products.iter().for_each(|(_, sub_rc)| {
            if sub_rc.is_some() {
                sub_rc
                    .as_ref()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .clear_dependencies(foliage)
            }
        });
    }

    pub fn set_dependencies(
        &mut self,
        foliage: &mut MutexGuard<Vec<Leaf>>,
        index_offset: Option<usize>,
        mut ancestors: Vec<usize>,
    ) {
        self.index = if index_offset.is_some() {
            index_offset.unwrap()
        } else {
            0
        };

        ancestors.push(self.index);

        self.products.iter().for_each(|(factors, _)| {
            factors.iter().for_each(|leaf| {
                foliage[*leaf as usize].add_dependency(self.index);
                ancestors
                    .iter()
                    .for_each(|ancestor| foliage[*leaf as usize].add_dependency(*ancestor));
            })
        });

        self.products.iter_mut().for_each(|(_, sub_rc)| {
            if sub_rc.is_some() {
                sub_rc.as_mut().unwrap().lock().unwrap().set_dependencies(
                    foliage,
                    Some(self.index + 1),
                    ancestors.clone(),
                )
            }
        });
    }

    pub fn drop_leaf(&mut self, leaf: u16) {
        self.products = self.products
            .par_iter()
            .map(|(factors, sub_rc)| {
                let mut factors_set = BTreeSet::from_iter(factors.clone());
                let mut new_rc: Option<SharedCircuit> = None;
                match factors_set.remove(&leaf) {
                    true => {
                        if sub_rc.is_some() {
                            let inner = sub_rc.as_ref().unwrap().lock().unwrap().clone();
                            new_rc = Some(Arc::new(Mutex::new(inner * leaf)));
                        } else {
                            new_rc = Some(Arc::new(Mutex::new(ReactiveCircuit::new() + leaf)));
                        }
                    }
                    false => match sub_rc {
                        Some(sub_rc) => sub_rc.lock().unwrap().drop_leaf(leaf),
                        None => (),
                    },
                }
                
                (Vec::from_iter(factors_set), new_rc)
            }).collect();
    }

    pub fn prune(&mut self) {
        // Keep track of redundant and newly created products
        let mut to_delete = BTreeSet::new();

        // We need to match factors between all products
        let original_length = self.products.len();
        for i in 0..original_length {
            let mut merged_rc = ReactiveCircuit::new();
            for j in i + 1..original_length {
                if to_delete.contains(&j) {
                    continue;
                }

                // If factors are equal, we can merge the circuits underneath
                let (factors_left, rc_left) = &self.products[i];
                let (factors_right, rc_right) = &self.products[j];
                if factors_left == factors_right {
                    // Add up circuits beneath each product
                    if rc_left.is_some() && rc_right.is_some() {
                        // Only add i's part once
                        if !to_delete.contains(&i) {
                            merged_rc += rc_left.as_ref().unwrap().lock().unwrap().clone();
                        }

                        // Add right half
                        merged_rc += rc_right.as_ref().unwrap().lock().unwrap().clone();

                        // Later remove both of these since they are redundant now
                        to_delete.insert(i);
                        to_delete.insert(j);
                    }
                }
            }

            if !merged_rc.products.is_empty() {
                self.products.push((
                    self.products[i].0.clone(),
                    Some(Arc::new(Mutex::new(merged_rc))),
                ));
            }
        }

        // Remove all redundant products
        while let Some(index) = to_delete.pop_last() {
            self.products.remove(index);
        }

        // Prune all underneath
        self.products.par_iter_mut().for_each(|(_, rc)| {
            if rc.is_some() {
                rc.as_mut().unwrap().lock().unwrap().prune();
            }
        })
    }

    pub fn reset(&mut self) {
        // Set storage to 0.0
        self.storage = 0.0;

        // Reset all underneath
        self.products.par_iter_mut().for_each(|(_, rc)| {
            if rc.is_some() {
                rc.as_mut().unwrap().lock().unwrap().reset();
            }
        })
    }

    pub fn deploy(rc: &SharedCircuit) -> Vec<SharedCircuit> {
        let mut rcs = vec![rc.clone()];
        rc.lock().unwrap().products.iter().for_each(|(_, sub_rc)| {
            if sub_rc.is_some() {
                rcs.append(&mut ReactiveCircuit::deploy(sub_rc.as_ref().unwrap()));
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

        dot_text += &format!("rc_{index} [shape=rect, label=\"RC = {}\"]\n", self.storage);

        let mut last = index;
        for (factors_index, (factors, sub_circuit)) in self.products.iter().enumerate() {
            let mut names = "\\[".to_owned();
            for factor in factors {
                names += &foliage[*factor as usize].name.to_owned();
                names += ", ";
            }

            // Remove trailing comma
            if names.len() > 2 {
                names = names[0..names.len() - 2].to_owned();
            }

            names += "\\]";

            dot_text += &format!("rc_{index} -> factors_{index}_{factors_index} [dir=none]\n");
            dot_text +=
                &format!("factors_{index}_{factors_index} [shape=rect, label=\"Π {names}\"]\n");

            let sub_text;
            if sub_circuit.is_some() {
                let sub_circuit = sub_circuit.as_ref().unwrap();
                let next = last + 1;

                dot_text += &format!("factors_{index}_{factors_index} -> rc_{next} [dir=none]\n");
                (sub_text, last) = sub_circuit
                    .lock()
                    .unwrap()
                    .get_dot_text(Some(next), foliage);
                dot_text += &sub_text;
            }
        }

        (dot_text, last)
    }
}

impl Add for ReactiveCircuit {
    type Output = ReactiveCircuit;

    fn add(self, rhs: ReactiveCircuit) -> Self::Output {
        let mut sum = ReactiveCircuit::new();
        sum.storage = self.storage + rhs.storage;

        for (factors, sub_rc) in &self.products {
            sum.products.push((factors.clone(), sub_rc.clone()));
        }

        for (factors, sub_rc) in &rhs.products {
            sum.products.push((factors.clone(), sub_rc.clone()));
        }

        sum
    }
}

impl Add<ReactiveCircuit> for &ReactiveCircuit {
    type Output = ReactiveCircuit;

    fn add(self, rhs: ReactiveCircuit) -> Self::Output {
        let mut sum = ReactiveCircuit::new();
        sum.storage = self.storage + rhs.storage;

        for (factors, sub_rc) in &self.products {
            sum.products.push((factors.clone(), sub_rc.clone()));
        }

        for (factors, sub_rc) in &rhs.products {
            sum.products.push((factors.clone(), sub_rc.clone()));
        }

        sum
    }
}

impl Add<&ReactiveCircuit> for ReactiveCircuit {
    type Output = ReactiveCircuit;

    fn add(self, rhs: &ReactiveCircuit) -> Self::Output {
        let mut sum = ReactiveCircuit::new();
        sum.storage = self.storage + rhs.storage;

        for (factors, sub_rc) in &self.products {
            sum.products.push((factors.clone(), sub_rc.clone()));
        }

        for (factors, sub_rc) in &rhs.products {
            sum.products.push((factors.clone(), sub_rc.clone()));
        }

        sum
    }
}

impl AddAssign for ReactiveCircuit {
    fn add_assign(&mut self, rhs: ReactiveCircuit) {
        // Store the sum of both memorized values
        self.storage += rhs.storage;

        // Add up the products of rhs
        for (factors, sub_rc) in &rhs.products {
            self.products.push((factors.clone(), sub_rc.clone()));
        }
    }
}

impl Add<u16> for ReactiveCircuit {
    type Output = ReactiveCircuit;

    fn add(mut self, rhs: u16) -> Self::Output {
        self.products.push((vec![rhs], None));

        self
    }
}

impl Add<u16> for &ReactiveCircuit {
    type Output = ReactiveCircuit;

    fn add(self, rhs: u16) -> Self::Output {
        let mut sum = ReactiveCircuit::new();
        sum = sum + self;
        sum.products.push((vec![rhs], None));

        sum
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
        for (factors, _) in &mut self.products {
            factors.push(rhs);
        }

        self
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::channels::manager::Manager;
    use crate::circuit::update;

    use std::sync::{Arc, Mutex};

    #[test]
    fn test_rc() {
        let mut rc = ReactiveCircuit::new();
        let mut manager = Manager::new();

        manager.create_leaf("0", 0.5, 0.0);
        manager.create_leaf("1", 0.75, 0.0);
        manager.create_leaf("2", 0.25, 0.0);
        manager.create_leaf("3", 0.1, 0.0);

        rc = rc + 0 + vec![1, 2, 3];
        rc.update(&manager.get_values());
        assert_eq!(rc.counted_update(&manager.foliage.lock().unwrap()), 3);
        assert_eq!(rc.value(), 0.5 + 0.75 * 0.25 * 0.1);

        rc.drop_leaf(2);
        rc.full_update(&manager.foliage.lock().unwrap());
        assert_eq!(rc.counted_update(&manager.foliage.lock().unwrap()), 3);
        assert_eq!(rc.value(), 0.5 + 0.75 * 0.25 * 0.1);

        rc.set_dependencies(&mut manager.foliage.lock().unwrap(), None, vec![]);
        assert!(manager.foliage.lock().unwrap()[1].indices.contains(&0));
        assert!(!manager.foliage.lock().unwrap()[1].indices.contains(&1));
        assert!(manager.foliage.lock().unwrap()[2].indices.contains(&0));
        assert!(manager.foliage.lock().unwrap()[2].indices.contains(&1));

        update(&manager.foliage, &manager.rc_queue, 1, 0.1);
        update(&manager.foliage, &manager.rc_queue, 2, 0.3);

        let root = Arc::new(Mutex::new(rc));
        let foliage_guard = manager.foliage.lock().unwrap();
        let mut queue_guard = manager.rc_queue.lock().unwrap();
        let deploy = ReactiveCircuit::deploy(&root);

        assert_eq!(queue_guard.len(), 2);
        assert_eq!(*queue_guard.first().unwrap(), 0);
        assert_eq!(*queue_guard.last().unwrap(), 1);
        while let Some(rc_index) = queue_guard.pop_last() {
            deploy[rc_index].lock().unwrap().update(&manager.get_values());
        }
        assert_eq!(deploy[0].lock().unwrap().value(), 0.5 + 0.1 * 0.3 * 0.1);
        assert_eq!(deploy[0].lock().unwrap().counted_update(&foliage_guard), 3);
        assert_eq!(deploy[1].lock().unwrap().value(), 0.3);
        assert_eq!(deploy[1].lock().unwrap().counted_update(&foliage_guard), 0);

        drop(foliage_guard);
        drop(queue_guard);
        assert_eq!(root.lock().unwrap().value(), 0.5 + 0.1 * 0.3 * 0.1);
    }

    #[test]
    fn test_dot_text() -> std::io::Result<()> {
        // Create basic RC
        let mut rc = ReactiveCircuit::new();
        let mut manager = Manager::new();

        manager.create_leaf("a", 0.5, 0.0);
        manager.create_leaf("b", 0.5, 0.0);
        manager.create_leaf("c", 0.5, 0.0);
        manager.create_leaf("d", 0.5, 0.0);

        // Test for a simple multiplication (rc = a * b)
        rc = rc + vec![0, 1] + vec![1, 2, 3] + vec![0, 2, 3];
        rc.full_update(&manager.foliage.lock().unwrap());
        let mut expected_text = "\
            rc_0 [shape=rect, label=\"RC = 0.5\"]\n\
            rc_0 -> factors_0_0 [dir=none]\n\
            factors_0_0 [shape=rect, label=\"Π \\[a, b\\]\"]\n\
            rc_0 -> factors_0_1 [dir=none]\n\
            factors_0_1 [shape=rect, label=\"Π \\[b, c, d\\]\"]\n\
            rc_0 -> factors_0_2 [dir=none]\n\
            factors_0_2 [shape=rect, label=\"Π \\[a, c, d\\]\"]\n\
        ";
        rc.to_svg(
            "output/plots/dot_test_1.svg",
            &manager.foliage.lock().unwrap(),
        )?;
        assert_eq!(
            rc.get_dot_text(None, &manager.foliage.lock().unwrap()).0,
            expected_text
        );

        rc.drop_leaf(1);
        rc.full_update(&manager.foliage.lock().unwrap());
        expected_text = "\
            rc_0 [shape=rect, label=\"RC = 0.5\"]\n\
            rc_0 -> factors_0_0 [dir=none]\n\
            factors_0_0 [shape=rect, label=\"Π \\[a\\]\"]\n\
            factors_0_0 -> rc_1 [dir=none]\n\
            rc_1 [shape=rect, label=\"RC = 0.5\"]\n\
            rc_1 -> factors_1_0 [dir=none]\n\
            factors_1_0 [shape=rect, label=\"Π \\[b\\]\"]\n\
            rc_0 -> factors_0_1 [dir=none]\n\
            factors_0_1 [shape=rect, label=\"Π \\[c, d\\]\"]\n\
            factors_0_1 -> rc_2 [dir=none]\n\
            rc_2 [shape=rect, label=\"RC = 0.5\"]\n\
            rc_2 -> factors_2_0 [dir=none]\n\
            factors_2_0 [shape=rect, label=\"Π \\[b\\]\"]\n\
            rc_0 -> factors_0_2 [dir=none]\n\
            factors_0_2 [shape=rect, label=\"Π \\[a, c, d\\]\"]\n\
        ";
        rc.to_svg(
            "output/plots/dot_test_2.svg",
            &manager.foliage.lock().unwrap(),
        )?;
        assert_eq!(
            rc.get_dot_text(None, &manager.foliage.lock().unwrap()).0,
            expected_text
        );

        rc.drop_leaf(0);
        rc.prune();
        rc.full_update(&manager.foliage.lock().unwrap());
        expected_text = "\
            rc_0 [shape=rect, label=\"RC = 0.5\"]\n\
            rc_0 -> factors_0_0 [dir=none]\n\
            factors_0_0 [shape=rect, label=\"Π \\[\\]\"]\n\
            factors_0_0 -> rc_1 [dir=none]\n\
            rc_1 [shape=rect, label=\"RC = 0.25\"]\n\
            rc_1 -> factors_1_0 [dir=none]\n\
            factors_1_0 [shape=rect, label=\"Π \\[a, b\\]\"]\n\
            rc_0 -> factors_0_1 [dir=none]\n\
            factors_0_1 [shape=rect, label=\"Π \\[c, d\\]\"]\n\
            factors_0_1 -> rc_2 [dir=none]\n\
            rc_2 [shape=rect, label=\"RC = 1\"]\n\
            rc_2 -> factors_2_0 [dir=none]\n\
            factors_2_0 [shape=rect, label=\"Π \\[b\\]\"]\n\
            rc_2 -> factors_2_1 [dir=none]\n\
            factors_2_1 [shape=rect, label=\"Π \\[a\\]\"]\n\
        ";
        rc.to_svg(
            "output/plots/dot_test_3.svg",
            &manager.foliage.lock().unwrap(),
        )?;
        assert_eq!(
            rc.get_dot_text(None, &manager.foliage.lock().unwrap()).0,
            expected_text
        );

        // Value should be unchanged
        assert_eq!(rc.value(), 0.5);

        Ok(())
    }
}
