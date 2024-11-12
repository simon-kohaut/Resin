#![allow(dead_code)]

mod channels;
mod circuit;
mod language;
mod tracking;

// use crate::channels::clustering::create_boundaries;
// use crate::channels::manager::Manager;
// use crate::circuit::ReactiveCircuit;

// use itertools::Itertools;
// use rand::prelude::*;
// use rand::seq::SliceRandom;
// use rand_distr::{Distribution, SkewNormal};

// use std::fs::File;
// use std::fs::OpenOptions;
// use std::io::Write;
// use std::path::Path;
// use std::sync::{Arc, Mutex};
// use std::time::Instant;
// use std::vec;

// pub fn power_set<T: Clone>(leafs: &[T]) -> Vec<Vec<T>> {
//     let mut power_set = Vec::new();
//     for i in 0..leafs.len() + 1 {
//         for set in leafs.iter().cloned().combinations(i) {
//             power_set.push(set);
//         }
//     }
//     power_set
// }

// pub fn random_set(number_leafs: u16, number_sets: usize) -> Vec<Vec<u16>> {
//     let mut random_set = Vec::new();

//     let mut rng = StdRng::seed_from_u64(0);
//     for _ in 0..number_sets {
//         random_set.push(
//             (0..number_leafs)
//                 .collect_vec()
//                 .choose_multiple(&mut rng, number_leafs as usize / 2)
//                 .cloned()
//                 .collect(),
//         );
//     }
//     random_set
// }

// fn randomized_rc(
//     manager: &mut Manager,
//     number_leafs: u16,
//     number_models: usize,
// ) -> ReactiveCircuit {
//     let mut rc = ReactiveCircuit::new();
//     for i in 0..number_leafs {
//         manager.create_leaf(&i.to_string(), 0.0, 0.0);
//     }

//     // let combinations = power_set(&(0..number_leafs).collect_vec());
//     let combinations = random_set(number_leafs, number_models);
//     for combination in combinations {
//         rc.add_leafs(combination);
//     }

//     rc
// }

// fn randomized_study(location: f64, bin_size: f64) {
//     // Model size
//     let number_leafs = 2000;
//     let number_models = 10000;

//     // How long to run each model
//     let inference_time = 30.0;

//     // Frequency distribution
//     let scale = 1.0;
//     let shape = 0.0;

//     // Partitioning of leafs
//     let number_bins = 500;
//     let boundaries = create_boundaries(bin_size, number_bins);

//     println!("Building randomized RC for location {location} and bin size {bin_size}.");
//     let mut manager = Manager::new();
//     let mut rc = randomized_rc(&mut manager, number_leafs, number_models);

//     println!("Activate randomized IPC.");
//     let mut true_frequencies = sample_frequencies(location, scale, shape, number_leafs as usize);
//     for (index, frequency) in true_frequencies.iter_mut().enumerate() {
//         if *frequency < 0.1 {
//             *frequency = 0.1;
//         }

//         let channel = format!(
//             "leaf_{}",
//             manager.foliage.lock().unwrap()[index as usize].name
//         );
//         let _ = manager.read(index as u16, &channel, false);
//         let _ = manager.make_timed_writer(&channel, *frequency);
//     }

//     let mut inference_timestamps = vec![];
//     let mut inference_times = vec![];
//     let mut values = vec![];

//     println!("Loop original for {}s.", inference_time);
//     let inference_clock = Instant::now();
//     while inference_clock.elapsed().as_secs_f64() < inference_time {
//         manager.spin_once();

//         let leaf_values = manager.get_values();

//         let mut queue_guard = manager.rc_queue.lock().unwrap();
//         if queue_guard.len() == 0 {
//             continue;
//         }

//         let before = Instant::now();
//         if let Some(_) = queue_guard.pop_last() {
//             rc.update(&leaf_values);
//         }
//         let elapsed = before.elapsed().as_secs_f64();
//         drop(queue_guard);

//         inference_timestamps.push(inference_clock.elapsed().as_secs_f64());
//         inference_times.push(elapsed);
//         values.push(rc.value());
//     }
//     println!(
//         "Original RC had value {} with depth {} in {} operations using {} Bytes",
//         rc.value(),
//         rc.depth(None),
//         rc.counted_update(&manager.get_values()),
//         rc.size()
//     );

//     println!("Export results.");
//     let path = Path::new("output/data/original_inference_times.csv");
//     if !path.exists() {
//         let mut file = File::create(path).expect("Unable to create file");
//         file.write_all("Time,Runtime,Leafs,Shape,Location,Value,BinSize,Size\n".as_bytes())
//             .expect("Unable to write data");
//     }

//     let mut file = OpenOptions::new().append(true).open(path).unwrap();
//     let mut csv_text = "".to_string();
//     for i in 0..inference_times.len() {
//         csv_text.push_str(&format!(
//             "{},{},{},{shape},{location},{},{bin_size},{}\n",
//             inference_timestamps[i],
//             inference_times[i],
//             number_leafs as usize / 2 * number_models as usize,
//             values[i],
//             rc.size()
//         ));
//     }
//     file.write_all(csv_text.as_bytes())
//         .expect("Unable to write data");

//     println!("Start adaptation");
//     let before = Instant::now();
//     // Adapt layers
//     // frequency_adaptation(&mut rc, &true_frequencies, &boundaries);

//     // Update leaf dependencies
//     rc.full_update(&manager.get_values());
//     println!("#Adaptations in {}s", before.elapsed().as_secs_f64());

//     let mut inference_timestamps = vec![];
//     let mut inference_times = vec![];
//     let mut values = vec![];

//     let root = Arc::new(Mutex::new(rc));
//     // let deploy = ReactiveCircuit::deploy(&root, &manager, None);

//     println!("Loop deployed for {}s.", inference_time);
//     let inference_clock = Instant::now();
//     while inference_clock.elapsed().as_secs_f64() < inference_time {
//         manager.spin_once();

//         let leaf_values = manager.get_values();
//         let mut queue_guard = manager.rc_queue.lock().unwrap();
//         if queue_guard.len() == 0 {
//             continue;
//         }

//         let before = Instant::now();
//         while let Some(rc_index) = queue_guard.pop_last() {
//             // deploy[rc_index].lock().unwrap().update(&leaf_values);
//         }
//         let elapsed = before.elapsed().as_secs_f64();
//         drop(queue_guard);

//         inference_timestamps.push(inference_clock.elapsed().as_secs_f64());
//         inference_times.push(elapsed);
//         values.push(root.lock().unwrap().value());
//     }
//     let root_value = root.lock().unwrap().value();
//     let root_depth = root.lock().unwrap().depth(None);
//     let root_ops = root.lock().unwrap().counted_update(&manager.get_values());
//     let graph_size = root.lock().unwrap().size();
//     println!("Adapted RC had value {root_value} with depth {root_depth} in {root_ops} operations using {graph_size} Bytes");

//     println!("Export results.");
//     let path = Path::new("output/data/adapted_inference_times.csv");
//     if !path.exists() {
//         let mut file = File::create(path).expect("Unable to create file");
//         file.write_all("Time,Runtime,Leafs,Shape,Location,Value,BinSize,Depth,Size\n".as_bytes())
//             .expect("Unable to write data");
//     }

//     let mut file = OpenOptions::new().append(true).open(path).unwrap();
//     let mut csv_text = "".to_string();
//     for i in 0..inference_times.len() {
//         csv_text.push_str(&format!(
//             "{},{},{},{shape},{location},{},{bin_size},{},{}\n",
//             inference_timestamps[i],
//             inference_times[i],
//             number_leafs as usize / 2 * number_models as usize,
//             values[i],
//             root_depth,
//             graph_size
//         ));
//     }
//     file.write_all(csv_text.as_bytes())
//         .expect("Unable to write data");
// }

// fn sample_frequencies(location: f64, scale: f64, shape: f64, number_samples: usize) -> Vec<f64> {
//     let distribution = SkewNormal::new(location, scale, shape).unwrap();
//     let mut rng = StdRng::seed_from_u64(0);

//     let mut frequencies = vec![];
//     while frequencies.len() < number_samples {
//         let frequency = distribution.sample(&mut rng).clamp(0.0001, f64::MAX);
//         frequencies.push(frequency);
//     }

//     frequencies
// }

// fn export_frequencies(path: &Path, location: f64, scale: f64, shape: f64, number_samples: usize) {
//     let frequencies = sample_frequencies(location, scale, shape, number_samples);

//     if !path.exists() {
//         let mut file = File::create(path).expect("Unable to create file");
//         file.write_all("Frequency,Location,Scale,Shape\n".as_bytes())
//             .expect("Unable to write data");
//     }

//     let mut file = OpenOptions::new().append(true).open(path).unwrap();
//     let mut csv_text = "".to_string();
//     for frequency in frequencies {
//         csv_text.push_str(&format!("{frequency},{location},{scale},{shape}\n"));
//     }

//     file.write_all(csv_text.as_bytes())
//         .expect("Unable to write data");
// }

// fn main() -> std::io::Result<()> {
//     let locations = vec![1.0, 5.0, 10.0];

//     for _ in 0..10 {
//         for location in &locations {
//             let mut bin_size = 1.0;
//             while bin_size <= 10.0 {
//                 randomized_study(*location, bin_size);
//                 bin_size += 1.0;
//             }
//         }
//     }

//     Ok(())
// }

fn main() -> std::io::Result<()> {
    Ok(())
}
