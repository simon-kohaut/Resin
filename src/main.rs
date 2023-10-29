#![allow(dead_code)]

mod circuit;
mod frequencies;
mod language;
mod tracking;

use std::io::{stdout, Write};

use crate::circuit::ipc::{retreive_messages, RandomizedIpcChannel};
use crate::circuit::Leaf;
use crate::circuit::Mul;

use circuit::leaf::activate_channel;
use circuit::RC;

use itertools::Itertools;
use linfa::prelude::SingleTargetRegression;
use linfa::traits::Transformer;
use linfa::Dataset;
use linfa_clustering::Dbscan;
use ndarray::{array, Array1, Array2};

use plotly::common::{AxisSide, Font, Title};
use plotly::layout::{Axis as PAxis, Layout};
use plotly::{Plot, Scatter};
use rand::seq::{index, SliceRandom};

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

pub fn random_set<T: Clone>(leafs: &[T]) -> Vec<Vec<T>> {
    let mut random_set = Vec::new();
    for i in 0..leafs.len() + 1 {
        for _ in 0..leafs.len() + 1 {
            random_set.push(
                leafs
                    .choose_multiple(&mut rand::thread_rng(), i)
                    .cloned()
                    .collect(),
            );
        }
    }
    random_set
}

fn randomized_rc(number_leafs: usize) -> RC {
    let mut rc = RC::new();
    for i in 0..number_leafs {
        rc.grow(0.0, &i.to_string());
    }

    let combinations = power_set(&(0..number_leafs).collect_vec());
    for combination in combinations {
        if combination.is_empty() {
            continue;
        }

        rc.add(Mul::new(combination));
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

    let mut frequencies: Vec<f64> = indexed_frequencies_pairs
        .iter()
        .map(|(_, leaf)| leaf.get_frequency())
        .collect();

    let dataset = Dataset::new(
        Array2::from_shape_vec((frequencies.len(), 1), frequencies).unwrap(),
        array![0.0],
    );

    let clusters = Dbscan::params(2).tolerance(0.5).transform(dataset);

    let mut cluster_counter = 0;
    let mut previous_cluster = 0;
    let mut cluster_steps = vec![];

    let mut foliage_guard = rc.foliage.lock().unwrap();
    for index in 0..clusters.records.shape()[0] {
        match clusters.targets[index] {
            Some(cluster) => {
                if cluster == previous_cluster {
                    cluster_steps.push(
                        foliage_guard[indexed_frequencies_pairs[index].0]
                            .set_cluster(&cluster_counter),
                    );
                } else {
                    previous_cluster = cluster;
                    cluster_counter += 1;
                    cluster_steps.push(
                        foliage_guard[indexed_frequencies_pairs[index].0]
                            .set_cluster(&cluster_counter),
                    );
                }
            }
            None => {
                previous_cluster = usize::MAX;
                cluster_counter += 1;
                cluster_steps.push(
                    foliage_guard[indexed_frequencies_pairs[index].0].set_cluster(&cluster_counter),
                );
            }
        }
    }
    println!("Clusters {:?}", clusters.targets);
    println!("Cluster steps {:?}", cluster_steps);
    drop(foliage_guard);

    if cluster_steps.iter().all(|step| *step == 0) {
        return;
    }

    rc.clear_dependencies();
    for (index, cluster_step) in cluster_steps.iter().enumerate() {
        if cluster_step != &0 {
            if cluster_step > &0 {
                for _ in 0..*cluster_step {
                    rc.disperse(indexed_frequencies_pairs[index].0);
                }
            } else {
                for _ in 0..-*cluster_step {
                    rc.collect(indexed_frequencies_pairs[index].0);
                }
            }
        }
    }
    rc.update_dependencies();
}

pub fn message_loop() {
    std::thread::spawn(move || -> Result<(), rclrs::RclrsError> {
        loop {
            retreive_messages();
        }
    });
}

fn randomized_study() {
    println!("Building randomized RC.");
    let number_leafs = 5;
    let mut rc = randomized_rc(number_leafs);

    println!("Activate randomized IPC.");
    print!("F = {{");
    let mut true_frequencies = vec![];
    for index in 0..number_leafs {
        let channel = format!("leaf_{}", rc.foliage.lock().unwrap()[index].name);
        activate_channel(rc.foliage.clone(), index, &channel, &false);

        use rand::Rng;
        let mut rng = rand::thread_rng();
        true_frequencies.push(rng.gen_range(0.001..5.0));
        let new_publisher = RandomizedIpcChannel::new(
            &rc.foliage.lock().unwrap()[index]
                .ipc_channel
                .as_ref()
                .unwrap()
                .topic,
            true_frequencies[true_frequencies.len() - 1],
            rng.gen_range(0.1..1.0),
        );
        print!("{}, ", new_publisher.as_ref().unwrap().frequency);
        new_publisher.unwrap().start();
    }
    let true_frequencies_array = Array1::from(true_frequencies);
    println!("}}");

    let mut operations = vec![];
    let mut operation_ratios = vec![];
    let mut values = vec![];
    let mut max_operations = 0.0;
    let mut mse = vec![];

    let mut inference_times = vec![];
    let mut inference_timestamps = vec![];
    let mut adaptation_times = vec![];
    let mut adaptation_timestamps = vec![];

    let experiment_time = 10;

    println!("Loop for {}s.", experiment_time);
    message_loop();
    let runtime_clock = Instant::now();
    loop {
        let before = Instant::now();
        let (value, n_ops) = rc.counted_value();
        inference_times.push(before.elapsed().as_secs_f64());
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

    let runtime_clock = Instant::now();
    loop {
        let before = Instant::now();
        let (value, n_ops) = rc.counted_value();
        inference_times.push(before.elapsed().as_secs_f64());
        inference_timestamps.push(runtime_clock.elapsed().as_secs_f64() + experiment_time as f64);

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

    println!("Evaluate sliding inference time.");
    let window_size = 1000;
    let sliding_times: Vec<f64> = inference_times
        .windows(window_size)
        .map(|window| window.iter().sum::<f64>() / window.len() as f64)
        .collect();
    let sliding_ratios: Vec<f64> = operation_ratios
        .windows(window_size)
        .map(|window| window.iter().sum::<f64>() / window.len() as f64)
        .collect();

    println!("Export results.");
    let mut plot = Plot::new();
    plot.add_trace(
        Scatter::new(inference_timestamps[window_size..].to_vec(), sliding_ratios)
            .name("Operations Ratio"),
    );
    plot.add_trace(
        Scatter::new(inference_timestamps.clone(), values.clone())
            .y_axis("y2")
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
                    .title(Title::new("Avg. Operations Ratio"))
                    .range(vec![0, 1]),
            )
            .y_axis2(
                PAxis::new()
                    .title(Title::new("RC Value").font(Font::new().color("#ff7f0e")))
                    .tick_font(Font::new().color("#ff7f0e"))
                    .anchor("free")
                    .overlaying("y")
                    .side(AxisSide::Right)
                    .position(1.0),
            ),
    );
    plot.write_html("output/operations.html");

    let mut plot = Plot::new();
    plot.add_trace(Scatter::new(inference_timestamps.clone(), mse.clone()));
    plot.set_layout(
        Layout::new()
            .title(Title::new("Reactive Inference"))
            .x_axis(
                PAxis::new()
                    .title(Title::new("Time / s"))
                    .range(vec![0, 2 * experiment_time]),
            )
            .y_axis(PAxis::new().title(Title::new("MSE of Estimated FoC"))),
    );
    plot.write_html("output/mse.html");

    plot = Plot::new();
    plot.add_trace(
        Scatter::new(inference_timestamps[window_size..].to_vec(), sliding_times)
            .name("Avg. Inference"),
    );
    plot.add_trace(
        Scatter::new(adaptation_timestamps.clone(), adaptation_times.clone())
            .name("Adaptation")
            .y_axis("y2"),
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
            .y_axis2(
                PAxis::new()
                    .title(
                        Title::new("Frequency Adaptation Time / s")
                            .font(Font::new().color("#ff7f0e")),
                    )
                    .tick_font(Font::new().color("#ff7f0e"))
                    .anchor("free")
                    .overlaying("y")
                    .side(AxisSide::Right)
                    .position(1.0),
            ),
    );
    plot.write_html("output/time.html");
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
