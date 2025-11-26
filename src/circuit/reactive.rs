// Standard library
use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::Write;
use std::mem::size_of_val;
use std::process::Command;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

// Crate
use crate::Manager;

pub type RcQueue = Arc<Mutex<BTreeSet<usize>>>;
pub type SharedCircuit = Arc<Mutex<ReactiveCircuit>>;

#[derive(Clone)]
pub struct ReactiveCircuit {
    pub storage: f64,
    pub products: HashMap<Vec<u16>, SharedCircuit>,
    pub index: usize,
    pub layer: usize,
}

pub struct DeployedCircuit {
    pub index: usize,
    pub products: HashMap<Vec<u16>, usize>,
}

impl DeployedCircuit {
    pub fn new(index: usize, products: HashMap<Vec<u16>, usize>) -> Self {
        Self { index, products }
    }

    pub fn update(&self, leafs: &[f64], storage: &mut Vec<f64>) -> f64 {
        // Empty Circuits return a neutral element
        if self.products.is_empty() {
            return 1.0;
        }

        self.products.iter().fold(0.0, |acc, (factors, rc)| {
            acc + factors
                .iter()
                .fold(storage[*rc], |acc, index| acc * leafs[*index as usize])
        })
    }
}

impl ReactiveCircuit {
    pub fn new() -> Self {
        Self {
            storage: 0.0,
            products: HashMap::new(),
            index: 0,
            layer: 0,
        }
    }

    pub fn one() -> Self {
        Self {
            storage: 1.0,
            products: HashMap::new(),
            index: 0,
            layer: 0,
        }
    }

    pub fn share(&self) -> SharedCircuit {
        Arc::new(Mutex::new(self.clone()))
    }

    pub fn deep_clone(&self) -> ReactiveCircuit {
        let mut rc = ReactiveCircuit::new();
        rc.index = self.index;
        rc.layer = self.layer;
        rc.storage = self.storage;

        let product_clone_iter = self.products.iter().map(|(factors, sub_rc)| {
            (factors.clone(), sub_rc.lock().unwrap().deep_clone().share())
        });
        rc.products = HashMap::from_iter(product_clone_iter);

        rc
    }

    pub fn deploy(&self) -> DeployedCircuit {
        let products = self
            .products
            .iter()
            .map(|(factors, rc)| (factors.clone(), rc.lock().unwrap().index));

        DeployedCircuit::new(self.index, HashMap::from_iter(products))
    }

    pub fn add_rc(&mut self, other: &SharedCircuit) {
        // Get locked access to other RC
        let rhs = other.lock().unwrap();

        // Store the sum of both memorized values
        self.storage += rhs.storage;

        for (factors, sub_rc) in &rhs.products {
            // Merge sub-circuitry if factors are the same, meaning
            // factors * a + factors * b = factors * (a + b)
            if self.products.contains_key(factors) {
                // Merg sub-circuitry by addition
                self.products
                    .get_mut(factors)
                    .unwrap()
                    .lock()
                    .unwrap()
                    .add_rc(sub_rc);
            } else {
                self.products.insert(factors.clone(), sub_rc.clone());
            }
        }
    }

    pub fn add_leafs(&mut self, mut leafs: Vec<u16>) {
        // Ensure leafs are sorted
        leafs.sort();

        // Either add 1 to sub-circuitry if leaf combination is already in products
        // or insert an entire new product
        if self.products.contains_key(&leafs) {
            self.products
                .get(&leafs)
                .unwrap()
                .lock()
                .unwrap()
                .add_rc(&ReactiveCircuit::one().share());
        } else {
            self.products.insert(leafs, ReactiveCircuit::one().share());
        }
    }

    pub fn add_leaf(&mut self, leaf: u16) {
        let factors = vec![leaf];
        self.add_leafs(factors);
    }

    pub fn mul_leaf(&mut self, leaf: u16) {
        let mut multiplied_products = HashMap::new();

        // If this is a const 1, we just insert the leaf
        if self.products.is_empty() {
            self.products
                .insert(vec![leaf], ReactiveCircuit::one().share());
            return;
        }

        for (factors, rc) in &mut self.products {
            let mut multiplied_factors = vec![leaf];
            multiplied_factors.append(&mut factors.clone());
            multiplied_factors.sort();

            multiplied_products.insert(multiplied_factors, rc.clone());
        }

        self.products = multiplied_products;
    }

    pub fn recompute_index(&mut self, mut index_offset: usize, layer_offset: usize) -> usize {
        // Empty RC do not get an index
        if self.products.is_empty() {
            return index_offset - 1;
        }

        // Set own index as cureent offset
        self.index = index_offset;
        self.layer = layer_offset;

        // The next child will have offset + 1 and return an offset representing the number of RCs beneath it as well
        // Meaning the next child will have an offset + 1 + n
        for (_, rc) in &self.products {
            index_offset = rc
                .lock()
                .unwrap()
                .recompute_index(index_offset + 1, layer_offset + 1);
        }

        index_offset
    }

    pub fn full_update(&mut self, leaf_values: &[f64]) {
        self.storage = self
            .products
            .iter()
            .map(|(factors, rc)| {
                rc.lock().unwrap().full_update(leaf_values);
                let value = rc.lock().unwrap().value();

                factors
                    .iter()
                    .map(|index| leaf_values[*index as usize])
                    .product::<f64>()
                    * value
            })
            .sum::<f64>();
    }

    pub fn counted_update(&mut self, leaf_values: &[f64]) -> usize {
        // Compute sum-product and count all additions and multiplications
        let count;
        (self.storage, count) =
            self.products
                .iter()
                .fold((0.0, 0), |(acc_value, acc_count), (factors, sub_rc)| {
                    // Get product of leafs
                    let mut value = factors
                        .iter()
                        .fold(1.0, |acc, factor| acc * leaf_values[*factor as usize]);

                    // How many numbers where multiplied
                    let mut count = if factors.len() > 0 {
                        factors.len() - 1
                    } else {
                        0
                    };

                    // Factor in the result of ReactiveCircuit underneath
                    if !sub_rc.lock().unwrap().products.is_empty() {
                        count += sub_rc.lock().unwrap().counted_update(leaf_values) + 1;
                        value *= sub_rc.lock().unwrap().value();
                    }

                    (acc_value + value, acc_count + count)
                });

        // Account for the additions separately
        count + self.products.len() - 1
    }

    pub fn update(&mut self, leaf_values: &[f64]) {
        let value = self.products.iter().fold(0.0, |acc, (factors, sub_rc)| {
            let value = sub_rc.lock().unwrap().value();

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
                let rc_guard = rc.lock().unwrap();
                if rc_guard.products.len() == 0 {
                    depth
                } else {
                    rc_guard.depth(Some(depth + 1))
                }
            })
            .max()
            .unwrap()
    }

    pub fn drop_leaf(&mut self, leaf: u16, repeat: usize) {
        let drop_iter = self.products.iter().map(|(factors, sub_rc)| {
            let mut factors_set = BTreeSet::from_iter(factors.clone());
            match factors_set.remove(&leaf) {
                true => {
                    sub_rc.lock().unwrap().mul_leaf(leaf);

                    if repeat > 0 {
                        sub_rc.lock().unwrap().drop_leaf(leaf, repeat - 1);
                    }
                }
                false => {
                    sub_rc.lock().unwrap().drop_leaf(leaf, repeat);
                }
            }

            (Vec::from_iter(factors_set), sub_rc.clone())
        });

        self.products = HashMap::from_iter(drop_iter);
    }

    pub fn get_layer(rc: &SharedCircuit, depth: usize) -> Vec<SharedCircuit> {
        if rc.lock().unwrap().layer == depth {
            vec![rc.clone()]
        } else {
            rc.lock()
                .unwrap()
                .products
                .iter()
                .fold(vec![], |mut acc, (_, sub_rc)| {
                    let mut layer = ReactiveCircuit::get_layer(sub_rc, depth);
                    acc.append(&mut layer);
                    acc
                })
        }
    }

    // pub fn prune(rc: &SharedCircuit) {
    //     let rc_depth = rc.lock().unwrap().depth(None);
    //     for depth in 0..rc_depth {
    //         let layer = ReactiveCircuit::get_layer(rc, depth);

    //         // First pass to gather equal products and merge sub-rcs
    //         let mut pruned: HashMap<Vec<u16>, Arc<Mutex<ReactiveCircuit>>> = HashMap::new();
    //         for layer_rc in &layer {
    //             let rc_guard = layer_rc.lock().unwrap();
    //             for (factors, sub_rc) in &rc_guard.products {
    //                 if pruned.contains_key(factors) {
    //                     pruned.get(factors).unwrap().lock().unwrap().add_rc(&sub_rc);
    //                 } else {
    //                     pruned.insert(factors.clone(), sub_rc.clone());
    //                 }
    //             }
    //         }

    //         // Second pass to
    //         for (factors, sub_rc) in pruned.iter() {
    //             for layer_rc in &layer {
    //                 let mut rc_guard = layer_rc.lock().unwrap();
    //                 if rc_guard.products.contains_key(factors) {
    //                     *rc_guard.products.get_mut(factors).unwrap() = sub_rc.clone();
    //                 }
    //             }
    //         }
    //     }
    // }

    // pub fn prune(&mut self) {
    //     // Keep track of redundant and newly created products
    //     let mut to_delete = BTreeSet::new();

    //     // We need to match factors between all products
    //     let original_length = self.products.len();
    //     for i in 0..original_length {
    //         let mut merged_rc = ReactiveCircuit::new();
    //         for j in i + 1..original_length {
    //             if to_delete.contains(&j) {
    //                 continue;
    //             }

    //             // If factors are equal, we can merge the circuits underneath
    //             let (factors_left, rc_left) = &self.products[i];
    //             let (factors_right, rc_right) = &self.products[j];
    //             if BTreeSet::from_iter(factors_left) == BTreeSet::from_iter(factors_right) {
    //                 // Add up circuits beneath each product
    //                 if rc_left.is_some() && rc_right.is_some() {
    //                     // Only add i's part once
    //                     if !to_delete.contains(&i) {
    //                         merged_rc += rc_left.as_ref().unwrap().lock().unwrap().clone();
    //                     }

    //                     // Add right half
    //                     merged_rc += rc_right.as_ref().unwrap().lock().unwrap().clone();

    //                     // Later remove both of these since they are redundant now
    //                     to_delete.insert(i);
    //                     to_delete.insert(j);
    //                 }
    //             }
    //         }

    //         if !merged_rc.products.is_empty() {
    //             self.products.push((
    //                 self.products[i].0.clone(),
    //                 Some(Arc::new(Mutex::new(merged_rc))),
    //             ));
    //         }
    //     }

    //     // Remove all redundant products
    //     while let Some(index) = to_delete.pop_last() {
    //         self.products.remove(index);
    //     }

    //     // Prune all underneath
    //     self.products.par_iter_mut().for_each(|(_, rc)| {
    //         if rc.is_some() {
    //             rc.as_mut().unwrap().lock().unwrap().prune();
    //         }
    //     })
    // }

    pub fn leafs(&self) -> BTreeSet<u16> {
        let mut leafs = BTreeSet::new();
        for (factors, _) in &self.products {
            leafs.append(&mut BTreeSet::from_iter(factors.iter().cloned()));
        }

        leafs
    }

    pub fn size(&self) -> usize {
        let size_underneath = self.products.iter().fold(0, |acc, (factors, sub_rc)| {
            acc + size_of_val(factors) + size_of_val(&*factors) + sub_rc.lock().unwrap().size()
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

            if sub_circuit.lock().unwrap().products.len() > 0 {
                let sub_text;
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

#[cfg(test)]
mod tests {

    use rand::Rng;
    use rand::prelude::IndexedRandom;
    use std::ops::RangeInclusive;

    use super::*;
    use crate::channels::clustering::{
        binning, create_boundaries, frequency_adaptation, pack, partitioning,
    };
    use crate::channels::manager::Manager;
    use crate::circuit::update;
    use crate::sample_frequencies;

    fn random_products(number_leafs: u16, number_sets: usize) -> Vec<Vec<u16>> {
        let mut random_products = Vec::new();

        let mut rng = rand::rng();
        for _ in 0..number_sets {
            random_products.push(
                (0..number_leafs)
                    .collect::<Vec<u16>>()
                    .choose_multiple(&mut rng, number_leafs as usize / 2) // Pass as mutable reference
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
        let mut rng = rand::rng();
        for i in 0..number_leafs {
            manager.create_leaf(&i.to_string(), rng.random_range(range.clone()), 0.0);
        }

        // Add up random combinations of the leafs
        let products = random_products(number_leafs, number_models);
        for product in products {
            rc.add_leafs(product);
        }

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
        let _ = rc.to_svg("output/test/random_rc_flat.svg", &manager);
        let partitions = partitioning(&manager.get_frequencies(), &boundaries);
        frequency_adaptation(&mut rc, &partitions);

        // RC should still compute same value
        rc.full_update(&manager.get_values());
        let _ = rc.to_svg("output/test/random_rc_adapted.svg", &manager);
        assert!((rc.value() - value).abs() < 1e-14);
        assert_eq!(rc.depth(None), *pack(&bins).iter().max().unwrap() + 1);

        // Deployed RC should still compute same value
        rc.recompute_index(0, 0);
        let root = rc.share();
        // let deployed = ReactiveCircuit::deploy(&root, &manager, None);

        // Update a leaf with a new value
        update(&manager.foliage, &manager.rc_queue, 1, 0.0, 0.0);

        // There should be invalidated RCs in the queue,
        // and the RC should still have the value from before stored
        assert!(manager.rc_queue.lock().unwrap().len() > 0);
        assert!((root.lock().unwrap().value() - value).abs() < 1e-14);

        // Update the necessary RCs
        // let leaf_values = manager.get_values();
        // while let Some(index) = manager.rc_queue.lock().unwrap().pop_last() {
        //     deployed[index].lock().unwrap().update(&leaf_values);
        // }

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
        let partitions = partitioning(&manager.get_frequencies(), &boundaries);
        frequency_adaptation(&mut rc, &partitions);

        // This should still be flat
        assert_eq!(rc.depth(None), 1);
    }

    #[test]
    fn test_rc() {
        let mut rc = ReactiveCircuit::new();
        let mut manager = Manager::new();
        let mut rng = rand::rng();

        let mut values = vec![
            rng.random_range(0.0..1.0),
            rng.random_range(0.0..1.0),
            rng.random_range(0.0..1.0),
            rng.random_range(0.0..1.0),
        ];
        manager.create_leaf("0", values[0], 0.0);
        manager.create_leaf("1", values[1], 0.0);
        manager.create_leaf("2", values[2], 0.0);
        manager.create_leaf("3", values[3], 0.0);

        let mut desired_value = values[0] + values[1] * values[2] * values[3];

        rc.add_leaf(0);
        rc.add_leafs(vec![1, 2, 3]);
        rc.update(&manager.get_values());
        assert_eq!(rc.counted_update(&manager.get_values()), 3);
        assert_eq!(rc.value(), desired_value);

        rc.drop_leaf(2, 0);
        rc.full_update(&manager.get_values());
        let _ = rc.to_svg("output/test/test_rc.svg", &manager);
        assert_eq!(rc.counted_update(&manager.get_values()), 3);
        assert!((rc.value() - desired_value).abs() < 1e-14);

        rc.recompute_index(0, 0);
        let root = rc.share();
        // let deploy = ReactiveCircuit::deploy(&root, &manager, None);

        assert_eq!(manager.foliage.lock().unwrap()[0].indices.len(), 1);
        assert_eq!(manager.foliage.lock().unwrap()[1].indices.len(), 1);
        assert_eq!(manager.foliage.lock().unwrap()[2].indices.len(), 2);
        assert_eq!(manager.foliage.lock().unwrap()[3].indices.len(), 1);

        values[0] = rng.random_range(0.0..1.0);
        values[2] = rng.random_range(0.0..1.0);
        desired_value = values[0] + values[1] * values[2] * values[3];
        update(&manager.foliage, &manager.rc_queue, 0, values[0], 0.0);
        update(&manager.foliage, &manager.rc_queue, 2, values[2], 0.0);

        let queue_guard = manager.rc_queue.lock().unwrap();
        assert_eq!(queue_guard.len(), 2);
        assert_eq!(*queue_guard.first().unwrap(), 0);
        // while let Some(rc_index) = queue_guard.pop_last() {
        //     deploy[rc_index]
        //         .lock()
        //         .unwrap()
        //         .update(&manager.get_values());
        // }
        // assert_eq!(deploy[0].lock().unwrap().value(), desired_value);
        // assert_eq!(
        //     deploy[0]
        //         .lock()
        //         .unwrap()
        //         .counted_update(&manager.get_values()),
        //     3
        // );
        // assert_eq!(deploy[1].lock().unwrap().value(), values[2]);
        // assert_eq!(
        //     deploy[1]
        //         .lock()
        //         .unwrap()
        //         .counted_update(&manager.get_values()),
        //     0
        // );

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

        // Test for a simple multiplication
        rc.add_leafs(vec![0, 1]);
        rc.add_leafs(vec![1, 2, 3]);
        rc.add_leafs(vec![0, 2, 3]);
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
