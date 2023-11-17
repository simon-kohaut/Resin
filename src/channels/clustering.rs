use crate::circuit::leaf::{Foliage, Leaf};
use crate::circuit::reactive::ReactiveCircuit;

pub fn binning(frequencies: &[f64], boundaries: &[f64]) -> Vec<usize> {
    let mut labels = vec![];
    for frequency in frequencies {
        for (cluster, boundary) in boundaries.iter().enumerate() {
            if frequency <= boundary {
                labels.push(cluster);
                break;
            }
        }
    }

    labels
}

pub fn frequency_adaptation(rc: &mut ReactiveCircuit, foliage: &mut Foliage, boundaries: &[f64]) {
    let mut indexed_frequencies_pairs: Vec<(usize, Leaf)> = vec![];
    for (i, leaf) in foliage.lock().unwrap().iter().enumerate() {
        let position = indexed_frequencies_pairs.binary_search_by(|pair| {
            pair.1
                .get_frequency()
                .partial_cmp(&leaf.get_frequency())
                .unwrap()
        });
        match position {
            Ok(position) => indexed_frequencies_pairs.insert(position, (i, leaf.clone())),
            Err(position) => indexed_frequencies_pairs.insert(position, (i, leaf.clone())),
        }
    }

    let frequencies: Vec<f64> = indexed_frequencies_pairs
        .iter()
        .map(|(_, leaf)| leaf.get_frequency())
        .collect();

    let mut cluster_steps = vec![];
    let mut foliage_guard = foliage.lock().unwrap();
    for (index, frequency) in frequencies.iter().enumerate() {
        for (cluster, boundary) in boundaries.iter().enumerate() {
            if *frequency <= *boundary {
                cluster_steps.push(
                    foliage_guard[indexed_frequencies_pairs[index].0]
                        .set_cluster(&(cluster as i32)),
                );
                break;
            }
        }
    }
    drop(foliage_guard);

    if cluster_steps.iter().all(|step| *step == 0) {
        return;
    }

    let min_cluster = cluster_steps.iter().min().unwrap().clone();
    for step in &mut cluster_steps {
        *step -= min_cluster;
    }

    for (index, cluster_step) in cluster_steps.iter().enumerate() {
        if cluster_step != &0 {
            if cluster_step > &0 {
                rc.drop_leaf(indexed_frequencies_pairs[index].0 as u16);
            } else {
                panic!("Not implemented!");
            }
        }
    }

    // Prune resulting RC
    rc.prune();

    // Update leaf dependencies
    rc.clear_dependencies(&mut foliage.lock().unwrap());
    rc.set_dependencies(&mut foliage.lock().unwrap(), None, vec![]);

    // Validate full circuit once
    rc.full_update(&foliage.lock().unwrap());
}
