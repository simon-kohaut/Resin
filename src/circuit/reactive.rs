// Standard library
use std::collections::BTreeSet;
use std::fs::File;
use std::io::Write;
use std::mem::size_of_val;
use std::ops::{Add, AddAssign, Mul};
use std::process::Command;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

// Third-party
use rayon::prelude::{IntoParallelRefIterator, IntoParallelRefMutIterator, ParallelIterator};

// Crate
use crate::Manager;

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

    pub fn share(&mut self) -> SharedCircuit {
        Arc::new(Mutex::new(self.clone()))
    }

    pub fn full_update(&mut self, leaf_values: &[f64]) {
        self.storage = self
            .products
            .iter_mut()
            .map(|(factors, rc)| {
                let value = if rc.is_some() {
                    rc.as_mut()
                        .unwrap()
                        .lock()
                        .unwrap()
                        .full_update(&leaf_values);
                    rc.as_mut().unwrap().lock().unwrap().value()
                } else {
                    1.0
                };

                factors
                    .iter()
                    .map(|index| leaf_values[*index as usize])
                    .product::<f64>()
                    * value
            })
            .sum::<f64>();
    }

    pub fn counted_update(&mut self, leaf_values: &[f64]) -> usize {
        let (value, count) =
            self.products
                .iter_mut()
                .fold((0.0, 0), |(acc_value, acc_count), (factors, sub_rc)| {
                    // Get product of leafs
                    let mut value = factors
                        .iter()
                        .fold(1.0, |acc, factor| acc * leaf_values[*factor as usize]);

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
        let value = self.products.iter().fold(0.0, |acc, (factors, sub_rc)| {
            let value;
            unsafe {
                value = match sub_rc {
                    Some(rc) => rc.lock().unwrap_unchecked().value(),
                    None => 1.0,
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

    pub fn depth(&self, offset: Option<usize>) -> usize {
        let depth = match offset {
            Some(offset) => offset,
            None => 1,
        };

        self.products
            .iter()
            .map(|(_, rc)| {
                if rc.is_some() {
                    rc.as_ref().unwrap().lock().unwrap().depth(Some(depth + 1))
                } else {
                    depth
                }
            })
            .max()
            .unwrap()
    }

    pub fn clear_dependencies(&self, manager: &mut Manager) {
        for leaf in manager.foliage.lock().unwrap().iter_mut() {
            leaf.remove_dependency(self.index);
        }

        self.products.iter().for_each(|(_, sub_rc)| {
            if sub_rc.is_some() {
                sub_rc
                    .as_ref()
                    .unwrap()
                    .lock()
                    .unwrap()
                    .clear_dependencies(manager)
            }
        });
    }

    pub fn set_dependencies(
        &mut self,
        manager: &mut Manager,
        index_offset: Option<usize>,
        mut ancestors: Vec<usize>,
    ) {
        self.index = if index_offset.is_some() {
            index_offset.unwrap()
        } else {
            0
        };
        ancestors.push(self.index);

        let mut foliage = manager.foliage.lock().unwrap();
        self.products.iter().for_each(|(factors, _)| {
            factors.iter().for_each(|leaf| {
                foliage[*leaf as usize].add_dependency(self.index);
                ancestors
                    .iter()
                    .for_each(|ancestor| foliage[*leaf as usize].add_dependency(*ancestor));
            })
        });
        drop(foliage);

        self.products.iter_mut().for_each(|(_, rc)| {
            if rc.is_some() {
                rc.as_mut().unwrap().lock().unwrap().set_dependencies(
                    manager,
                    Some(self.index + 1),
                    ancestors.clone(),
                )
            }
        });
    }

    pub fn flatten(&mut self) {
        todo!()
    }

    pub fn drop_leaf(&mut self, leaf: u16, repeat: usize) {
        self.products = self
            .products
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

                        if repeat > 0 {
                            new_rc
                                .as_mut()
                                .unwrap()
                                .lock()
                                .unwrap()
                                .drop_leaf(leaf, repeat - 1);
                        }

                        new_rc.as_mut().unwrap().lock().unwrap().prune();
                    }
                    false => match sub_rc {
                        Some(rc) => {
                            rc.lock().unwrap().drop_leaf(leaf, repeat);
                            new_rc = sub_rc.clone();
                        }
                        None => (),
                    },
                }

                (Vec::from_iter(factors_set), new_rc)
            })
            .collect();
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
                if BTreeSet::from_iter(factors_left) == BTreeSet::from_iter(factors_right) {
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

    pub fn size(&self) -> usize {
        let size_underneath = self.products.iter().fold(0, |acc, (factors, sub_rc)| {
            acc + size_of_val(factors)
                + size_of_val(&*factors)
                + if sub_rc.is_some() {
                    sub_rc.as_ref().unwrap().lock().unwrap().size()
                } else {
                    0
                }
        });

        size_of_val(&self.storage)
            + size_of_val(&self.products)
            + size_underneath
            + size_of_val(&self.index)
    }

    pub fn to_svg(&self, path: &str, manager: &Manager) -> std::io::Result<()> {
        let mut dot_text = String::from_str("strict digraph {\nnode [shape=circle]\n").unwrap();
        dot_text += &self.get_dot_text(Some(0), manager).0;
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

    pub fn get_dot_text(&self, index: Option<u16>, manager: &Manager) -> (String, u16) {
        let mut dot_text = String::new();
        let index = if index.is_some() { index.unwrap() } else { 0 };

        dot_text += &format!("rc_{index} [shape=rect, label=\"RC = {}\"]\n", self.storage);

        let mut last = index;
        for (factors_index, (factors, sub_circuit)) in self.products.iter().enumerate() {
            let mut names = "\\[".to_owned();
            let foliage = manager.foliage.lock().unwrap();
            for factor in factors {
                names += &foliage[*factor as usize].name.to_owned();
                names += ", ";
            }
            drop(foliage);

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
                    .get_dot_text(Some(next), manager);
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
        for product in &rhs.products {
            self.products.push(product.clone());
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

    use itertools::Itertools;
    use rand::{seq::SliceRandom, thread_rng, Rng};
    use std::ops::RangeInclusive;
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::channels::clustering::{binning, create_boundaries, frequency_adaptation, pack};
    use crate::channels::manager::Manager;
    use crate::circuit::update;
    use crate::sample_frequencies;

    fn random_products(number_leafs: u16, number_sets: usize) -> Vec<Vec<u16>> {
        let mut random_products = Vec::new();

        let mut rng = thread_rng();
        for _ in 0..number_sets {
            random_products.push(
                (0..number_leafs)
                    .collect_vec()
                    .choose_multiple(&mut rng, number_leafs as usize / 2)
                    .cloned()
                    .collect(),
            );
        }

        random_products
    }

    fn random_rc(
        manager: &mut Manager,
        number_leafs: u16,
        number_models: usize,
        range: RangeInclusive<f64>,
    ) -> ReactiveCircuit {
        // Create empty RC
        let mut rc = ReactiveCircuit::new();

        // Create leafs
        let mut rng = thread_rng();
        for i in 0..number_leafs {
            manager.create_leaf(&i.to_string(), rng.gen_range(range.clone()), 0.0);
        }

        // Add up random combinations of the leafs
        let products = random_products(number_leafs, number_models);
        for product in products {
            rc = rc + product;
        }

        rc.set_dependencies(manager, None, vec![]);
        rc
    }

    #[test]
    fn test_random_rc() {
        // Setup runtime environment
        let mut manager = Manager::new();

        // Create a decently large WMC problem
        let number_leafs = 20;
        let number_models = 100;
        let mut rc = random_rc(&mut manager, number_leafs, number_models, 0.1..=1.0);
        assert_eq!(rc.depth(None), 1);

        // Compute RC value which should not change during any of the following steps
        rc.full_update(&manager.get_values());
        let value = rc.value();

        // Make a random adapation of the tree
        let location = 5.0;
        let scale = 2.0;
        let frequencies = sample_frequencies(location, scale, 0.0, number_leafs as usize);
        let boundaries = create_boundaries(1.0, 20);
        let bins = binning(&frequencies, &boundaries);

        // Set frequencies and adapt RC
        for (index, leaf) in manager.foliage.lock().unwrap().iter_mut().enumerate() {
            leaf.set_frequency(&frequencies[index]);
        }
        frequency_adaptation(&mut rc, &manager.get_frequencies(), &boundaries);
        rc.clear_dependencies(&mut manager);
        rc.set_dependencies(&mut manager, None, vec![]);

        // RC should still compute same value
        rc.full_update(&manager.get_values());
        assert!((rc.value() - value).abs() < 1e-14);
        assert_eq!(rc.depth(None), *pack(&bins).iter().max().unwrap() + 1);

        // Deployed RC should still compute same value
        let root = rc.share();
        let deployed = ReactiveCircuit::deploy(&root);

        // Update a leaf with a new value
        update(&manager.foliage, &manager.rc_queue, 1, 0.0);

        // There should be invalidated RCs in the queue,
        // and the RC should still have the value from before stored
        assert!(manager.rc_queue.lock().unwrap().len() > 0);
        assert!((root.lock().unwrap().value() - value).abs() < 1e-14);

        // Update the necessary RCs
        let leaf_values = manager.get_values();
        while let Some(index) = manager.rc_queue.lock().unwrap().pop_last() {
            deployed[index].lock().unwrap().update(&leaf_values);
        }

        // The RC should now have a different value
        assert_ne!(root.lock().unwrap().value(), value);
    }

    #[test]
    fn test_wide_bin_size() {
        // Setup runtime environment
        let mut manager = Manager::new();

        // Create a decently large WMC problem
        let number_leafs = 20;
        let number_models = 100;
        let mut rc = random_rc(&mut manager, number_leafs, number_models, 0.0..=1.0);
        assert_eq!(rc.depth(None), 1);

        // Make a random adapation of the tree but choose boundaries such that only a single bin should be filled
        let location = 5.0;
        let scale = 2.0;
        let frequencies = sample_frequencies(location, scale, 0.0, number_leafs as usize);
        let boundaries = create_boundaries(10.0, 20);

        // Set frequencies and adapt RC
        for (index, leaf) in manager.foliage.lock().unwrap().iter_mut().enumerate() {
            leaf.set_frequency(&frequencies[index]);
        }
        frequency_adaptation(&mut rc, &manager.get_frequencies(), &boundaries);

        // This should still be flat
        assert_eq!(rc.depth(None), 1);
    }

    #[test]
    fn test_rc() {
        let mut rc = ReactiveCircuit::new();
        let mut manager = Manager::new();
        let mut rng = thread_rng();

        let mut values = vec![
            rng.gen_range(0.0..1.0),
            rng.gen_range(0.0..1.0),
            rng.gen_range(0.0..1.0),
            rng.gen_range(0.0..1.0),
        ];
        manager.create_leaf("0", values[0], 0.0);
        manager.create_leaf("1", values[1], 0.0);
        manager.create_leaf("2", values[2], 0.0);
        manager.create_leaf("3", values[3], 0.0);

        rc = rc + 0 + vec![1, 2, 3];
        let mut desired_value = values[0] + values[1] * values[2] * values[3];
        rc.update(&manager.get_values());
        assert_eq!(rc.counted_update(&manager.get_values()), 3);
        assert_eq!(rc.value(), desired_value);

        rc.drop_leaf(2, 0);
        rc.full_update(&manager.get_values());
        assert_eq!(rc.counted_update(&manager.get_values()), 3);
        assert!((rc.value() - desired_value).abs() < 1e-14);

        rc.set_dependencies(&mut manager, None, vec![]);
        assert!(manager.foliage.lock().unwrap()[1].indices.contains(&0));
        assert!(!manager.foliage.lock().unwrap()[1].indices.contains(&1));
        assert!(manager.foliage.lock().unwrap()[2].indices.contains(&0));
        assert!(manager.foliage.lock().unwrap()[2].indices.contains(&1));

        values[0] = rng.gen_range(0.0..1.0);
        values[2] = rng.gen_range(0.0..1.0);
        desired_value = values[0] + values[1] * values[2] * values[3];
        update(&manager.foliage, &manager.rc_queue, 0, values[0]);
        update(&manager.foliage, &manager.rc_queue, 2, values[2]);

        let root = Arc::new(Mutex::new(rc));
        let mut queue_guard = manager.rc_queue.lock().unwrap();
        let deploy = ReactiveCircuit::deploy(&root);

        assert_eq!(queue_guard.len(), 2);
        assert_eq!(*queue_guard.first().unwrap(), 0);
        assert_eq!(*queue_guard.last().unwrap(), 1);
        while let Some(rc_index) = queue_guard.pop_last() {
            deploy[rc_index]
                .lock()
                .unwrap()
                .update(&manager.get_values());
        }
        assert_eq!(deploy[0].lock().unwrap().value(), desired_value);
        assert_eq!(
            deploy[0]
                .lock()
                .unwrap()
                .counted_update(&manager.get_values()),
            3
        );
        assert_eq!(deploy[1].lock().unwrap().value(), values[2]);
        assert_eq!(
            deploy[1]
                .lock()
                .unwrap()
                .counted_update(&manager.get_values()),
            0
        );

        drop(queue_guard);
        assert_eq!(root.lock().unwrap().value(), desired_value);
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
        rc.full_update(&manager.get_values());
        let mut expected_text = "\
            rc_0 [shape=rect, label=\"RC = 0.5\"]\n\
            rc_0 -> factors_0_0 [dir=none]\n\
            factors_0_0 [shape=rect, label=\"Π \\[a, b\\]\"]\n\
            rc_0 -> factors_0_1 [dir=none]\n\
            factors_0_1 [shape=rect, label=\"Π \\[b, c, d\\]\"]\n\
            rc_0 -> factors_0_2 [dir=none]\n\
            factors_0_2 [shape=rect, label=\"Π \\[a, c, d\\]\"]\n\
        ";
        rc.to_svg("output/plots/dot_test_1.svg", &manager)?;
        assert_eq!(rc.get_dot_text(None, &manager).0, expected_text);

        rc.drop_leaf(1, 0);
        rc.full_update(&manager.get_values());
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
        rc.to_svg("output/plots/dot_test_2.svg", &manager)?;
        assert_eq!(rc.get_dot_text(None, &manager).0, expected_text);

        rc.drop_leaf(0, 0);
        rc.prune();
        rc.full_update(&manager.get_values());
        expected_text = "\
            rc_0 [shape=rect, label=\"RC = 0.5\"]\n\
            rc_0 -> factors_0_0 [dir=none]\n\
            factors_0_0 [shape=rect, label=\"Π \\[\\]\"]\n\
            factors_0_0 -> rc_1 [dir=none]\n\
            rc_1 [shape=rect, label=\"RC = 0.25\"]\n\
            rc_1 -> factors_1_0 [dir=none]\n\
            factors_1_0 [shape=rect, label=\"Π \\[b, a\\]\"]\n\
            rc_0 -> factors_0_1 [dir=none]\n\
            factors_0_1 [shape=rect, label=\"Π \\[c, d\\]\"]\n\
            factors_0_1 -> rc_2 [dir=none]\n\
            rc_2 [shape=rect, label=\"RC = 1\"]\n\
            rc_2 -> factors_2_0 [dir=none]\n\
            factors_2_0 [shape=rect, label=\"Π \\[b\\]\"]\n\
            rc_2 -> factors_2_1 [dir=none]\n\
            factors_2_1 [shape=rect, label=\"Π \\[a\\]\"]\n\
        ";
        rc.to_svg("output/plots/dot_test_3.svg", &manager)?;
        assert_eq!(rc.get_dot_text(None, &manager).0, expected_text);

        // Value should be unchanged
        assert_eq!(rc.value(), 0.5);

        Ok(())
    }
}
