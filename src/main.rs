#![allow(dead_code)]

mod channels;
mod circuit;
mod language;
mod tracking;

use crate::channels::clustering::create_boundaries;
use crate::channels::clustering::frequency_adaptation;
use crate::channels::manager::Manager;
use crate::circuit::ReactiveCircuit;

use itertools::Itertools;
use rand::prelude::*;
use rand::seq::SliceRandom;
use rand_distr::{Distribution, SkewNormal};

use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use std::vec;

pub fn power_set<T: Clone>(leafs: &[T]) -> Vec<Vec<T>> {
    let mut power_set = Vec::new();
    for i in 0..leafs.len() + 1 {
        for set in leafs.iter().cloned().combinations(i) {
            power_set.push(set);
        }
    }
    power_set
}

pub fn random_set(number_leafs: u16, number_sets: usize) -> Vec<Vec<u16>> {
    let mut random_set = Vec::new();

    let mut rng = StdRng::seed_from_u64(0);
    for _ in 0..number_sets {
        random_set.push(
            (0..number_leafs)
                .collect_vec()
                .choose_multiple(&mut rng, number_leafs as usize / 2)
                .cloned()
                .collect(),
        );
    }
    random_set
}

fn randomized_rc(
    manager: &mut Manager,
    number_leafs: u16,
    number_models: usize,
) -> ReactiveCircuit {
    let mut rc = ReactiveCircuit::new();
    for i in 0..number_leafs {
        manager.create_leaf(&i.to_string(), 0.0, 0.0);
    }

    // let combinations = power_set(&(0..number_leafs).collect_vec());
    let combinations = random_set(number_leafs, number_models);
    for combination in combinations {
        rc = rc + combination;
    }

    rc.set_dependencies(manager, None, vec![]);
    rc
}

// fn frequency_adaptation(rc: &mut ReactiveCircuit, foliage: &mut Foliage) {
//     let mut indexed_frequencies_pairs: Vec<(usize, Leaf)> = vec![];
//     for (i, leaf) in foliage.lock().unwrap().iter().enumerate() {
//         let position = indexed_frequencies_pairs.binary_search_by(|pair| {
//             pair.1
//                 .get_frequency()
//                 .partial_cmp(&leaf.get_frequency())
//                 .unwrap()
//         });
//         match position {
//             Ok(position) => indexed_frequencies_pairs.insert(position, (i, leaf.clone())),
//             Err(position) => indexed_frequencies_pairs.insert(position, (i, leaf.clone())),
//         }
//     }

//     let frequencies: Vec<f64> = indexed_frequencies_pairs
//         .iter()
//         .map(|(_, leaf)| leaf.get_frequency())
//         .collect();

//     // let dataset = Dataset::new(
//     //     Array2::from_shape_vec((frequencies.len(), 1), frequencies).unwrap(),
//     //     array![0.0],
//     // );

//     // let clusters = Dbscan::params(2).tolerance(0.02).transform(dataset);

//     // let mut cluster_counter = 0;
//     // let mut previous_cluster = 0;
//     let mut cluster_steps = vec![];

//     // let mut foliage_guard = rc.foliage.lock().unwrap();
//     // for index in 0..clusters.records.shape()[0] {
//     //     match clusters.targets[index] {
//     //         Some(cluster) => {
//     //             if cluster == previous_cluster {
//     //                 cluster_steps.push(
//     //                     foliage_guard[indexed_frequencies_pairs[index].0]
//     //                         .set_cluster(&cluster_counter),
//     //                 );
//     //             } else {
//     //                 previous_cluster = cluster;
//     //                 cluster_counter += 1;
//     //                 cluster_steps.push(
//     //                     foliage_guard[indexed_frequencies_pairs[index].0]
//     //                         .set_cluster(&cluster_counter),
//     //                 );
//     //             }
//     //         }
//     //         None => {
//     //             previous_cluster = usize::MAX;
//     //             cluster_counter += 1;
//     //             cluster_steps.push(
//     //                 foliage_guard[indexed_frequencies_pairs[index].0].set_cluster(&cluster_counter),
//     //             );
//     //         }
//     //     }
//     // }
//     // drop(foliage_guard);

//     let mut foliage_guard = foliage.lock().unwrap();
//     let boundaries = vec![0.01, 0.1, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 10.0, 100.0];
//     for (index, frequency) in frequencies.iter().enumerate() {
//         for (cluster, boundary) in boundaries.iter().enumerate() {
//             if *frequency <= *boundary {
//                 cluster_steps.push(
//                     foliage_guard[indexed_frequencies_pairs[index].0]
//                         .set_cluster(&(cluster as i32)),
//                 );
//                 break;
//             }
//         }
//     }
//     drop(foliage_guard);

//     if cluster_steps.iter().all(|step| *step == 0) {
//         return;
//     }

//     let min_cluster = cluster_steps.iter().min().unwrap().clone();
//     for step in &mut cluster_steps {
//         *step -= min_cluster;
//     }

//     // println!("Clusters {:?}", clusters.targets);
//     println!("Cluster steps {:?}", cluster_steps);

//     for (index, cluster_step) in cluster_steps.iter().enumerate() {
//         if cluster_step != &0 {
//             if cluster_step > &0 {
//                 rc.drop(
//                     indexed_frequencies_pairs[index].0 as u16,
//                     // *cluster_step as usize - 1,
//                 );
//             } else {
//                 panic!("Not implemented!");
//                 // rc.collect(
//                 //     indexed_frequencies_pairs[index].0 as u16,
//                 //     -*cluster_step as usize - 1,
//                 // );
//             }
//         }
//         if index % 100 == 0 {
//             println!("Done {index}");
//         }
//     }
//     rc.clear_dependencies(&mut foliage.lock().unwrap());
//     rc.set_dependencies(&mut foliage.lock().unwrap());
//     // rc.empty_scope();
// }

// pub fn message_loop(foliage: Foliage, ok: AtomicBool) -> JoinHandle<Result<(), rclrs::RclrsError>> {
//     std::thread::spawn(move || -> Result<(), rclrs::RclrsError> {
//         while ok.load(Acquire) {
//             let foliage_guard = foliage.lock().unwrap();
//             retreive_messages();
//             drop(foliage_guard);
//         }
//     })
// }

fn randomized_study(location: f64, bin_size: f64) {
    // Model size
    let number_leafs = 2000;
    let number_models = 10000;

    // How long to run each model
    let inference_time = 30.0;

    // Frequency distribution
    let scale = 1.0;
    let shape = 0.0;

    // Partitioning of leafs
    let number_bins = 500;
    let boundaries = create_boundaries(bin_size, number_bins);

    println!("Building randomized RC for location {location} and bin size {bin_size}.");
    let mut manager = Manager::new();
    let mut rc = randomized_rc(&mut manager, number_leafs, number_models);

    println!("Activate randomized IPC.");
    let mut true_frequencies = sample_frequencies(location, scale, shape, number_leafs as usize);
    for (index, frequency) in true_frequencies.iter_mut().enumerate() {
        if *frequency < 0.1 {
            *frequency = 0.1;
        }

        let channel = format!(
            "leaf_{}",
            manager.foliage.lock().unwrap()[index as usize].name
        );
        let _ = manager.read(index as u16, &channel, false);
        let _ = manager.write(&channel, *frequency);
    }

    let mut inference_timestamps = vec![];
    let mut inference_times = vec![];
    let mut values = vec![];

    println!("Loop original for {}s.", inference_time);
    let inference_clock = Instant::now();
    while inference_clock.elapsed().as_secs_f64() < inference_time {
        manager.spin_once();

        let leaf_values = manager.get_values();

        let mut queue_guard = manager.rc_queue.lock().unwrap();
        if queue_guard.len() == 0 {
            continue;
        }

        let before = Instant::now();
        if let Some(_) = queue_guard.pop_last() {
            rc.update(&leaf_values);
        }
        let elapsed = before.elapsed().as_secs_f64();
        drop(queue_guard);

        inference_timestamps.push(inference_clock.elapsed().as_secs_f64());
        inference_times.push(elapsed);
        values.push(rc.value());
    }
    println!(
        "Original RC had value {} with depth {} in {} operations using {} Bytes",
        rc.value(),
        rc.depth(None),
        rc.counted_update(&manager.get_values()),
        rc.size()
    );

    println!("Export results.");
    let path = Path::new("output/data/original_inference_times.csv");
    if !path.exists() {
        let mut file = File::create(path).expect("Unable to create file");
        file.write_all("Time,Runtime,Leafs,Shape,Location,Value,BinSize,Size\n".as_bytes())
            .expect("Unable to write data");
    }

    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    let mut csv_text = "".to_string();
    for i in 0..inference_times.len() {
        csv_text.push_str(&format!(
            "{},{},{},{shape},{location},{},{bin_size},{}\n",
            inference_timestamps[i],
            inference_times[i],
            number_leafs as usize / 2 * number_models as usize,
            values[i],
            rc.size()
        ));
    }
    file.write_all(csv_text.as_bytes())
        .expect("Unable to write data");

    println!("Start adaptation");
    let before = Instant::now();
    // Adapt layers
    frequency_adaptation(&mut rc, &true_frequencies, &boundaries);

    // Update leaf dependencies
    rc.clear_dependencies(&mut manager);
    rc.set_dependencies(&mut manager, None, vec![]);
    rc.full_update(&manager.get_values());
    println!("#Adaptations in {}s", before.elapsed().as_secs_f64());

    let mut inference_timestamps = vec![];
    let mut inference_times = vec![];
    let mut values = vec![];

    let root = Arc::new(Mutex::new(rc));
    let deploy = ReactiveCircuit::deploy(&root);

    println!("Loop deployed for {}s.", inference_time);
    let inference_clock = Instant::now();
    while inference_clock.elapsed().as_secs_f64() < inference_time {
        manager.spin_once();

        let leaf_values = manager.get_values();
        let mut queue_guard = manager.rc_queue.lock().unwrap();
        if queue_guard.len() == 0 {
            continue;
        }

        let before = Instant::now();
        while let Some(rc_index) = queue_guard.pop_last() {
            deploy[rc_index].lock().unwrap().update(&leaf_values);
        }
        let elapsed = before.elapsed().as_secs_f64();
        drop(queue_guard);

        inference_timestamps.push(inference_clock.elapsed().as_secs_f64());
        inference_times.push(elapsed);
        values.push(root.lock().unwrap().value());
    }
    let root_value = root.lock().unwrap().value();
    let root_depth = root.lock().unwrap().depth(None);
    let root_ops = root.lock().unwrap().counted_update(&manager.get_values());
    let graph_size = root.lock().unwrap().size();
    println!("Adapted RC had value {root_value} with depth {root_depth} in {root_ops} operations using {graph_size} Bytes",);

    println!("Export results.");
    let path = Path::new("output/data/adapted_inference_times.csv");
    if !path.exists() {
        let mut file = File::create(path).expect("Unable to create file");
        file.write_all("Time,Runtime,Leafs,Shape,Location,Value,BinSize,Depth,Size\n".as_bytes())
            .expect("Unable to write data");
    }

    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    let mut csv_text = "".to_string();
    for i in 0..inference_times.len() {
        csv_text.push_str(&format!(
            "{},{},{},{shape},{location},{},{bin_size},{},{}\n",
            inference_timestamps[i],
            inference_times[i],
            number_leafs as usize / 2 * number_models as usize,
            values[i],
            root_depth,
            graph_size
        ));
    }
    file.write_all(csv_text.as_bytes())
        .expect("Unable to write data");
}

fn sample_frequencies(location: f64, scale: f64, shape: f64, number_samples: usize) -> Vec<f64> {
    let distribution = SkewNormal::new(location, scale, shape).unwrap();
    let mut rng = StdRng::seed_from_u64(0);

    let mut frequencies = vec![];
    while frequencies.len() < number_samples {
        let frequency = distribution.sample(&mut rng).clamp(0.0001, f64::MAX);
        frequencies.push(frequency);
    }

    frequencies
}

fn export_frequencies(path: &Path, location: f64, scale: f64, shape: f64, number_samples: usize) {
    let frequencies = sample_frequencies(location, scale, shape, number_samples);

    if !path.exists() {
        let mut file = File::create(path).expect("Unable to create file");
        file.write_all("Frequency,Location,Scale,Shape\n".as_bytes())
            .expect("Unable to write data");
    }

    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    let mut csv_text = "".to_string();
    for frequency in frequencies {
        csv_text.push_str(&format!("{frequency},{location},{scale},{shape}\n"));
    }

    file.write_all(csv_text.as_bytes())
        .expect("Unable to write data");
}

fn main() -> std::io::Result<()> {
    let locations = vec![1.0, 5.0, 10.0];

    for _ in 0..10 {
        for location in &locations {
            let mut bin_size = 1.0;
            while bin_size <= 10.0 {
                randomized_study(*location, bin_size);
                bin_size += 1.0;
            }
        }
    }

    Ok(())

    // let args = Args::parse();

    // let model = read_to_string(args.source).unwrap();
    // let mut resin = compile(model);

    // for leaf in resin.leafs.values() {
    //     match &leaf.lock().unwrap().ipc_channel {
    //         Some(channel) => {
    //             use rand::Rng;
    //             let mut rng = rand::thread_rng();
    //             let new_publisher =
    //                 RandomizedIpcChannel::new(&channel.topic, rng.gen_range(0.1..50.0));
    //             new_publisher.unwrap().start();
    //         }
    //         None => (),
    //     }
    // }

    // let clock = SystemTime::now();
    // let mut operations = vec![];
    // let mut times = vec![];
    // loop {
    //     retreive_messages();
    //     frequency_adaptation(&mut resin);
    //     let (value, n_ops) = resin.circuits[0].lock().unwrap().get_value();
    //     operations.push(n_ops);

    //     match clock.elapsed() {
    //         Ok(elapsed) => {
    //             times.push(elapsed.as_secs_f64());
    //             if elapsed.as_secs() > 5 { break; }
    //         }
    //         Err(e) => {
    //             println!("Error {e:?}");
    //             break;
    //         }
    //     }
    // }

    // let mut plot = Plot::new();
    // plot.add_trace(Scatter::new(times, operations));
    // plot.set_layout(Layout::new()
    //     .title(Title::new("Sales Data"))
    //     .x_axis(PAxis::new().title(Title::new("Time / s")))
    //     .y_axis(PAxis::new().title(Title::new("#Operations")))
    // );
    // plot.write_html("output/operations_curve.html");

    // loop {
    //     println!("Value of RC = {:?}", resin.circuits[0].lock().unwrap().get_value());
    //     retreive_messages();
    // }

    // Ok(())
}
