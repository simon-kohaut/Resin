use crate::circuit::leaf::{Foliage, Leaf};
use crate::circuit::reactive::ReactiveCircuit;

pub fn binning(frequencies: &[f64], boundaries: &[f64]) -> Vec<usize> {
    // Append MAX for edge case of value being larger than any boundary
    let mut expanded_boundaries = boundaries.to_owned();
    expanded_boundaries.push(f64::MAX);

    // Set label once value <= boundary
    let mut labels = vec![];
    for frequency in frequencies {
        for (cluster, boundary) in expanded_boundaries.iter().enumerate() {
            if frequency <= boundary {
                labels.push(cluster);
                break;
            }
        }
    }

    // Return labels
    labels
}

pub fn create_boundaries(bin_size: f64, number_bins: usize) -> Vec<f64> {
    // Setup Vec of boundaries
    let mut boundaries = vec![];
    for i in 1..=number_bins {
        boundaries.push(i as f64 * bin_size);
    }

    // Return boundaries
    boundaries
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



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binning() {
        let frequencies = vec![1.0, 1.5, 2.25, 3.45, 45.0, 1000.0];
        let boundaries = vec![1.0, 2.0, 5.0, 10.0, 100.0, 999.0];

        let bins = binning(&frequencies, &boundaries);

        assert_eq!(bins, vec![0, 1, 2, 2, 4, 6]);
    }

    #[test]
    fn test_boundary_creation() {
        let bin_size = 3.0;
        let number_bins = 5;

        let boundaries = create_boundaries(bin_size, number_bins);

        assert_eq!(boundaries, vec![3.0, 6.0, 9.0, 12.0, 15.0]);
    }
}
