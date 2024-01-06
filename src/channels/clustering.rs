use std::collections::{BTreeSet, HashMap};

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

pub fn pack(bins: &[usize]) -> Vec<usize> {
    // Packed bins, e.g., go from [0, 2, 2, 3, 5] to [0, 1, 1, 2, 3]
    let mut packed_bins = vec![];

    // Keep track of what bin will be which bin in packed form
    let mut packing_assignment = HashMap::new();
    let mut number_packed_bins = 0;

    // Every time the bin is different from the one before,
    // assign a new bin
    for bin in BTreeSet::from_iter(bins) {
        if !packing_assignment.contains_key(bin) {
            packing_assignment.insert(bin, number_packed_bins);
            number_packed_bins += 1;
        }
    }

    // Built packed bins vec
    for bin in bins {
        packed_bins.push(*packing_assignment.get(bin).unwrap());
    }

    packed_bins
}

pub fn flip(bins: &[usize]) -> Vec<usize> {
    // Flipped bins, e.g., go from [0, 2, 2, 3, 5] to [5, 3, 3, 2, 0]
    let mut flipped = vec![];

    // Collect the absolute difference to the max value
    // In the example above, |2 - 5| = 3
    let max = bins.iter().max().unwrap().clone();
    for bin in bins {
        flipped.push(bin.abs_diff(max));
    }

    flipped
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

pub fn partitioning(frequencies: &[f64], boundaries: &[f64]) -> Vec<usize> {
    flip(&pack(&binning(&frequencies, boundaries)))
}

pub fn frequency_adaptation(
    rc: &mut ReactiveCircuit,
    partitioning: &[usize],
) -> i32 {
    // let mut indexed_frequencies_pairs: Vec<(usize, Leaf)> = vec![];
    // for (i, leaf) in foliage.lock().unwrap().iter().enumerate() {
    //     let position = indexed_frequencies_pairs.binary_search_by(|pair| {
    //         pair.1
    //             .get_frequency()
    //             .partial_cmp(&leaf.get_frequency())
    //             .unwrap()
    //     });
    //     match position {
    //         Ok(position) => indexed_frequencies_pairs.insert(position, (i, leaf.clone())),
    //         Err(position) => indexed_frequencies_pairs.insert(position, (i, leaf.clone())),
    //     }
    // }

    // let frequencies: Vec<f64> = indexed_frequencies_pairs
    //     .iter()
    //     .map(|(_, leaf)| leaf.get_frequency())
    //     .collect();

    // let mut cluster_steps = vec![];
    // let mut foliage_guard = foliage.lock().unwrap();
    // for (index, frequency) in frequencies.iter().enumerate() {
    //     for (cluster, boundary) in boundaries.iter().enumerate() {
    //         if *frequency <= *boundary {
    //             cluster_steps.push(
    //                 foliage_guard[indexed_frequencies_pairs[index].0]
    //                     .set_cluster(&(cluster as i32)),
    //             );
    //             break;
    //         }
    //     }
    // }

    // drop(foliage_guard);

    // if cluster_steps.iter().all(|step| *step == 0) {
    //     return;
    // }

    // let min_cluster = cluster_steps.iter().min().unwrap().clone();
    // for step in &mut cluster_steps {
    //     *step -= min_cluster;
    // }

    let mut number_of_adaptations = 0;
    for (index, cluster_step) in partitioning.iter().enumerate() {
        if *cluster_step != 0 {
            rc.drop_leaf(index as u16, *cluster_step - 1);
            number_of_adaptations += 1;
        }
    }

    number_of_adaptations
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binning() {
        // Check deterministic case
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

    #[test]
    fn test_packing() {
        let bins = vec![0, 1, 20, 3, 4, 4, 4, 8, 10];
        let packed = pack(&bins);
        assert_eq!(packed, vec![0, 1, 6, 2, 3, 3, 3, 4, 5]);
    }
}
