#![allow(dead_code)]

mod circuit;
mod frequencies;
mod language;
mod tracking;

use crate::circuit::ipc::{retreive_messages, RandomizedIpcChannel};
use crate::circuit::Leaf;
use crate::circuit::Mul;

use atomic_float::AtomicF64;
use circuit::leaf::{activate_channel, Foliage};
use circuit::RC;

use itertools::Itertools;
use linfa::prelude::SingleTargetRegression;
use linfa::traits::Transformer;
use linfa::Dataset;
use linfa_clustering::Dbscan;
use ndarray::{array, Array1, Array2};

use plotly::common::{AxisSide, Font, Title};
use plotly::layout::{Axis as PAxis, Layout};
use plotly::{Plot, Scatter, Bar};
use rand::seq::SliceRandom;
use rand::Rng;
use rand_distr::{Distribution, SkewNormal};
use rayon::prelude::{IntoParallelRefMutIterator, ParallelIterator};

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::sync::atomic::Ordering::{Acquire, Release};
use std::thread::JoinHandle;
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

pub fn random_set(number_leafs: usize, number_sets: usize) -> Vec<Vec<usize>> {
    let mut random_set = Vec::new();

    let mut rng = rand::thread_rng();
    for _ in 0..number_sets {
        random_set.push(
            (0..number_leafs)
                .collect_vec()
                .choose_multiple(&mut rng, number_leafs / 2)
                .cloned()
                .collect(),
        );
    }
    random_set
}

fn randomized_rc(number_leafs: usize) -> RC {
    let mut rc = RC::new();
    for i in 0..number_leafs {
        rc.grow(0.0, &i.to_string());
    }

    // let combinations = power_set(&(0..number_leafs).collect_vec());
    let combinations = random_set(number_leafs, 100);
    for combination in combinations {
        if combination.is_empty() {
            continue;
        }

        rc.add(Mul::new(combination.iter().map(|&e| e as u16).collect()));
    }

    rc.update_dependencies();
    rc
}

fn frequency_adaptation(rc: &mut RC) {
    let mut indexed_frequencies_pairs: Vec<(usize, Leaf)> = vec![];
    for (i, leaf) in rc.foliage.lock().unwrap().iter().enumerate() {
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

    // let dataset = Dataset::new(
    //     Array2::from_shape_vec((frequencies.len(), 1), frequencies).unwrap(),
    //     array![0.0],
    // );

    // let clusters = Dbscan::params(2).tolerance(0.02).transform(dataset);

    // let mut cluster_counter = 0;
    // let mut previous_cluster = 0;
    let mut cluster_steps = vec![];

    // let mut foliage_guard = rc.foliage.lock().unwrap();
    // for index in 0..clusters.records.shape()[0] {
    //     match clusters.targets[index] {
    //         Some(cluster) => {
    //             if cluster == previous_cluster {
    //                 cluster_steps.push(
    //                     foliage_guard[indexed_frequencies_pairs[index].0]
    //                         .set_cluster(&cluster_counter),
    //                 );
    //             } else {
    //                 previous_cluster = cluster;
    //                 cluster_counter += 1;
    //                 cluster_steps.push(
    //                     foliage_guard[indexed_frequencies_pairs[index].0]
    //                         .set_cluster(&cluster_counter),
    //                 );
    //             }
    //         }
    //         None => {
    //             previous_cluster = usize::MAX;
    //             cluster_counter += 1;
    //             cluster_steps.push(
    //                 foliage_guard[indexed_frequencies_pairs[index].0].set_cluster(&cluster_counter),
    //             );
    //         }
    //     }
    // }
    // drop(foliage_guard);

    let mut foliage_guard = rc.foliage.lock().unwrap();
    let boundaries = vec![0.01, 0.1, 1.0, 10.0, 100.0];
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

    // println!("Clusters {:?}", clusters.targets);
    println!("Cluster steps {:?}", cluster_steps);

    rc.clear_dependencies();
    for (index, cluster_step) in cluster_steps.iter().enumerate() {
        if cluster_step != &0 {
            if cluster_step > &0 {
                rc.disperse(
                    indexed_frequencies_pairs[index].0 as u16,
                    *cluster_step as usize - 1,
                );
            } else {
                rc.collect(
                    indexed_frequencies_pairs[index].0 as u16,
                    -*cluster_step as usize - 1,
                );
            }
        }
        if index % 100 == 0 {
            println!("Done {index}");
        }
    }
    rc.update_dependencies();
    rc.empty_scope();
}

// pub fn message_loop(foliage: Foliage, ok: AtomicBool) -> JoinHandle<Result<(), rclrs::RclrsError>> {
//     std::thread::spawn(move || -> Result<(), rclrs::RclrsError> {
//         while ok.load(Acquire) {
//             let foliage_guard = foliage.lock().unwrap();
//             retreive_messages();
//             drop(foliage_guard);
//         }
//     })
// }

fn randomized_study() {
    println!("Building randomized RC.");
    let experiment_time = 20;
    let number_leafs = 100;
    let mut rc = randomized_rc(number_leafs);

    println!("Activate randomized IPC.");
    let distribution = SkewNormal::new(0.1, 3.0, -1.0).unwrap();
    let mut true_frequencies = vec![];
    for index in 0..number_leafs {
        let channel = format!("leaf_{}", rc.foliage.lock().unwrap()[index].name);
        activate_channel(rc.foliage.clone(), index, &channel, &false);

        let mut rng = rand::thread_rng();
        let mut frequency = distribution.sample(&mut rng);
        if frequency < 0.001 {
            frequency = 0.001;
        }
        true_frequencies.push(frequency as f64);
        let new_publisher = RandomizedIpcChannel::new(
            &rc.foliage.lock().unwrap()[index]
                .ipc_channel
                .as_ref()
                .unwrap()
                .topic,
            true_frequencies[true_frequencies.len() - 1],
            rng.gen_range(0.1..1.0),
        );
        new_publisher.unwrap().start();
    }
    let mut sorted_frequencies = true_frequencies.clone();
    sorted_frequencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("F = {:?}", sorted_frequencies);
    let true_frequencies_array = Array1::from(true_frequencies);

    let mut operations = vec![];
    let mut operation_ratios = vec![];
    let mut values = vec![];
    let mut max_operations = 0.0;
    let mut mse = vec![];

    let mut inference_times = vec![];
    let mut inference_timestamps = vec![];
    let mut adaptation_times = vec![];
    let mut adaptation_timestamps = vec![];

    println!("Loop original for {}s.", experiment_time);
    // let _ = message_loop(rc.foliage.clone());

    let runtime_clock = Instant::now();
    loop {
        retreive_messages();

        let before = Instant::now();
        let (value, n_ops) = rc.counted_value();
        let elapsed = before.elapsed().as_secs_f64();

        if n_ops == 0 {
            continue;
        }

        // let value = rc.value();

        // println!("Inference took {elapsed}s");
        inference_times.push(elapsed);
        inference_timestamps.push(runtime_clock.elapsed().as_secs_f64());

        values.push(value);
        if operations.is_empty() {
            max_operations = n_ops as f64;
        }
        operations.push(n_ops);
        operation_ratios.push(n_ops as f64 / max_operations);

        let frequencies: Vec<f64> = rc
            .foliage
            .lock()
            .unwrap()
            .iter()
            .map(|leaf| leaf.get_frequency())
            .collect();

        mse.push(
            Array1::from(frequencies)
                .mean_squared_error(&true_frequencies_array)
                .unwrap(),
        );

        if runtime_clock.elapsed().as_secs() >= experiment_time {
            break;
        }
    }

    adaptation_timestamps.push(runtime_clock.elapsed().as_secs_f64());
    let before = Instant::now();
    frequency_adaptation(&mut rc);
    adaptation_times.push(before.elapsed().as_secs_f64());
    println!(
        "#Adaptations in {}s",
        adaptation_times[adaptation_times.len() - 1]
    );

    // println!("Loop adapted for {}s.", experiment_time);
    // let runtime_clock = Instant::now();
    // loop {
    //     retreive_messages();

    //     let before = Instant::now();
    //     let (value, n_ops) = rc.counted_value();
    //     let elapsed = before.elapsed().as_secs_f64();

    //     // if n_ops == 0 {
    //     //     continue;
    //     // }
    //     // println!("Inference took {elapsed}s");
    //     inference_times.push(elapsed);
    //     inference_timestamps.push(runtime_clock.elapsed().as_secs_f64() + experiment_time as f64);

    //     values.push(value);
    //     if operations.is_empty() {
    //         max_operations = n_ops as f64;
    //     }
    //     operations.push(n_ops);
    //     operation_ratios.push(n_ops as f64 / max_operations);

    //     if runtime_clock.elapsed().as_secs() >= experiment_time {
    //         break;
    //     }
    // }

    let mut deploy = rc.deploy();
    deploy.reverse();
    println!("Loop deployed for {experiment_time}s.");
    let runtime_clock = Instant::now();
    loop {
        retreive_messages();

        let before = Instant::now();
        let foliage_guard = rc.foliage.lock().unwrap();
        let n_ops = deploy.par_iter_mut().fold(|| 0, |acc, memory| acc + memory.counted_value(&foliage_guard).1).sum::<usize>();
        let value= deploy[0].value(&foliage_guard);
        drop(foliage_guard);
        let elapsed = before.elapsed().as_secs_f64();

        if n_ops == 0 {
            continue;
        }

        values.push(value);

        if operations.is_empty() {
            max_operations = n_ops as f64;
        }
        operations.push(n_ops);
        operation_ratios.push(n_ops as f64 / max_operations);

        inference_times.push(elapsed);
        inference_timestamps.push(runtime_clock.elapsed().as_secs_f64() + experiment_time as f64);

        if runtime_clock.elapsed().as_secs() >= experiment_time {
            break;
        }
    }

    let window_size = 30;
    let averaged_inference_times: Vec<f64> = inference_times
        .windows(window_size)
        .map(|w| w.iter().sum::<f64>() / w.len() as f64)
        .collect();

    println!("Export results.");
    let mut plot = Plot::new();
    plot.add_trace(
        Scatter::new(inference_timestamps.to_vec(), operation_ratios).name("Operations Ratio"),
    );
    plot.add_trace(
        Scatter::new(inference_timestamps.clone(), values.clone())
            .name("Value"),
    );
    plot.set_layout(
        Layout::new()
            .title(Title::new("Reactive Inference"))
            .x_axis(
                PAxis::new()
                    .title(Title::new("Time / s"))
                    .range(vec![0, 2 * experiment_time]),
            )
            .y_axis(
                PAxis::new()
                    .title(Title::new("Operations Ratio"))
                    .range(vec![0, 1]),
            )
    );
    plot.write_html("output/operations.html");

    let mut plot = Plot::new();
    plot.add_trace(
        Scatter::new(inference_timestamps.clone(), values.clone())
            .name("Value"),
    );
    plot.set_layout(
        Layout::new()
            .title(Title::new("Reactive Inference"))
            .x_axis(
                PAxis::new()
                    .title(Title::new("Time / s"))
                    .range(vec![0, 2 * experiment_time]),
            )
            .y_axis(
                PAxis::new()
                    .title(Title::new("RC Value"))
                    .range(vec![0, 1]),
            )
    );
    plot.write_html("output/values.html");
    // let mut plot = Plot::new();
    // plot.add_trace(Scatter::new(inference_timestamps.clone(), mse.clone()));
    // plot.set_layout(
    //     Layout::new()
    //         .title(Title::new("Reactive Inference"))
    //         .x_axis(
    //             PAxis::new()
    //                 .title(Title::new("Time / s"))
    //                 .range(vec![0, 2 * experiment_time]),
    //         )
    //         .y_axis(PAxis::new().title(Title::new("MSE of Estimated FoC"))),
    // );
    // plot.write_html("output/mse.html");

    plot = Plot::new();
    plot.add_trace(
        Scatter::new(inference_timestamps.to_vec(), inference_times).name("Inference Time"),
    );
    plot.add_trace(
        Scatter::new(
            inference_timestamps[window_size / 2..].to_vec(),
            averaged_inference_times.clone(),
        )
        .name("Avg. Inference Time"),
    );
    plot.set_layout(
        Layout::new()
            .title(Title::new("Reactive Inference"))
            .x_axis(
                PAxis::new()
                    .title(Title::new("Time / s"))
                    .range(vec![0, 2 * experiment_time]),
            )
            .y_axis(PAxis::new().title(Title::new("Inference Time / s")))
    );
    plot.write_html("output/time.html");

    // plot = Plot::new();
    // plot.add_trace(Bar::new(vec![0, 1], vec![sum_inference_before, sum_inference_after]));
    // plot.write_html("output/oaverall_time.html");
}

fn main() -> std::io::Result<()> {
    randomized_study();

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

    Ok(())

    // loop {
    //     println!("Value of RC = {:?}", resin.circuits[0].lock().unwrap().get_value());
    //     retreive_messages();
    // }

    // Ok(())
}
