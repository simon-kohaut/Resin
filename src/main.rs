#![allow(dead_code)]

mod circuit;
mod frequencies;
mod language;
mod tracking;

use crate::circuit::ipc::{retreive_messages, RandomizedIpcChannel};
use crate::circuit::reactive_circuit::get_root;
use crate::circuit::{compile, Args};
use crate::language::Resin;
use circuit::leaf::activate_channel;
use circuit::morphisms::prune;
use circuit::{shared_leaf, Model, ReactiveCircuit, SharedLeaf, SharedReactiveCircuit};
use clap::Parser;
use itertools::Itertools;
use linfa::prelude::SingleTargetRegression;
use linfa::traits::Transformer;
use linfa::Dataset;
use linfa_clustering::Dbscan;
use ndarray::{array, concatenate, Array1, Array2, Axis};
use plotly::common::Mode;
use plotly::common::{AxisSide, Font, Title};
use plotly::layout::{Axis as PAxis, Layout, Legend};
use plotly::{Plot, Scatter};
use rand::seq::SliceRandom;
use std::io::{stdin, stdout, Read, Write};
use std::process::exit;
use std::time::Instant;
use std::vec;
use std::{fs::read_to_string, process::Output};

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

fn randomized_rc(number_leafs: i32) -> (SharedReactiveCircuit, Vec<SharedLeaf>) {
    let mut leafs = vec![];
    for i in 0..number_leafs {
        let leaf = shared_leaf(0.0, 0.0, &i.to_string());
        leafs.push(leaf);
    }

    let rc = ReactiveCircuit::empty_new().share();
    let combinations = power_set(&leafs);
    for combination in combinations {
        if combination.len() == 0 {
            continue;
        }

        let _ = Model::new(&combination, &None, &Some(rc.clone()));
    }

    (rc, leafs)
}

fn frequency_adaptation(
    leafs: Vec<SharedLeaf>,
    circuits: Vec<SharedReactiveCircuit>,
) -> Vec<SharedReactiveCircuit> {
    let mut frequencies: Vec<f64> = leafs
        .iter()
        .map(|leaf| leaf.lock().unwrap().get_frequency())
        .collect();
    frequencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let dataset = Dataset::new(
        Array2::from_shape_vec((frequencies.len(), 1), frequencies).unwrap(),
        array![0.0],
    );

    let clusters = Dbscan::params(2).tolerance(0.5).transform(dataset);

    let mut cluster_counter = 0;
    let mut previous_cluster = 0;
    let mut cluster_step;

    let mut adapted_circuits = vec![];
    for i in 0..clusters.records.shape()[0] {
        match clusters.targets[i] {
            Some(cluster) => {
                if cluster == previous_cluster {
                    cluster_step = leafs[i].lock().unwrap().set_cluster(&cluster_counter);
                } else {
                    previous_cluster = cluster;
                    cluster_counter += 1;
                    cluster_step = leafs[i].lock().unwrap().set_cluster(&cluster_counter);
                }
            }
            None => {
                previous_cluster = usize::MAX;
                cluster_counter += 1;
                cluster_step = leafs[i].lock().unwrap().set_cluster(&cluster_counter);
            }
        }

        if cluster_step != 0 {
            for circuit in circuits.iter().cloned() {
                adapted_circuits.push(ReactiveCircuit::empty_new().share());
                if cluster_step > 0 {
                    for _ in 0..cluster_step {
                        let clock = Instant::now();
                        println!("Drop {} took {}s", i, clock.elapsed().as_secs_f64());
                        drop![adapted_circuits[0], &circuit.clone(), &leafs[i]];
                        let _ = circuit.lock().unwrap().to_svg(&format!(
                            "output/drop_{}_{}.svg",
                            i,
                            clock.elapsed().as_secs_f64()
                        ));
                        println!("New value = {:?}", circuit.lock().unwrap().get_value());
                    }
                } else {
                    for _ in 0..-cluster_step {
                        let clock = Instant::now();
                        println!("Lift {} took {}s", i, clock.elapsed().as_secs_f64());
                        lift![adapted_circuits[0], &circuit.clone(), &leafs[i]];
                        let _ = circuit.lock().unwrap().to_svg(&format!(
                            "output/lift_{}_{}.svg",
                            i,
                            clock.elapsed().as_secs_f64()
                        ));
                        println!("New value = {:?}", circuit.lock().unwrap().get_value());
                    }
                }
            }
        }
    }

    adapted_circuits
}

fn randomized_study() {
    println!("Building randomized RC.");
    let (rc, leafs) = randomized_rc(5);

    rc.lock().unwrap().to_svg("output/original.svg").unwrap();

    // lift![rc, &rc, &leafs[0]];
    // rc.lock().unwrap().to_svg("output/lift_0.svg");

    // lift![rc, &rc, &leafs[0]];
    // rc.lock().unwrap().to_svg("output/lift_00.svg");

    // exit(0);

    println!("Activate randomized IPC.");
    print!("F = {{");
    let mut true_frequencies = vec![];
    for leaf in &leafs {
        let channel = format!("leaf_{}", leaf.lock().unwrap().name);
        activate_channel(&leaf, &channel, &false);

        use rand::Rng;
        let mut rng = rand::thread_rng();
        true_frequencies.push(rng.gen_range(0.1..10.0));
        let new_publisher = RandomizedIpcChannel::new(
            &leaf.lock().unwrap().ipc_channel.as_ref().unwrap().topic,
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
    let runtime_clock = Instant::now();
    let mut adaptation_clock = Instant::now();
    let mut mse = vec![];

    let mut inference_times = vec![];
    let mut inference_timestamps = vec![];
    let mut adaptation_times = vec![];
    let mut adaptation_timestamps = vec![];
    let mut circuits = vec![rc.clone()];

    let experiment_time = 25;

    println!("Loop for {}s.", experiment_time);
    loop {
        retreive_messages();

        if adaptation_clock.elapsed().as_secs_f64() > 5.0 {
            adaptation_clock = Instant::now();
            adaptation_timestamps.push(runtime_clock.elapsed().as_secs_f64());
            let before = Instant::now();
            circuits = frequency_adaptation(leafs.clone(), circuits);
            adaptation_times.push(before.elapsed().as_secs_f64());
            println!(
                "#Adaptations in {}s",
                adaptation_times[adaptation_times.len() - 1]
            );
        }

        let before = Instant::now();
        let (value, n_ops) = circuits[0].lock().unwrap().get_value();
        if n_ops > 0 {
            inference_times.push(before.elapsed().as_secs_f64());
            inference_timestamps.push(runtime_clock.elapsed().as_secs_f64());

            if value == 31.0 {
                println!("Value = {}", value);
            }
            values.push(value);
            if operations.is_empty() {
                max_operations = n_ops as f64;
            }
            operations.push(n_ops);
            operation_ratios.push(n_ops as f64 / max_operations);

            let frequencies: Vec<f64> = leafs
                .iter()
                .map(|leaf| leaf.lock().unwrap().get_frequency())
                .collect();

            mse.push(
                Array1::from(frequencies)
                    .mean_squared_error(&true_frequencies_array)
                    .unwrap(),
            );
        }

        if runtime_clock.elapsed().as_secs() >= experiment_time {
            break;
        }
    }

    println!("Evaluate sliding inference time.");
    let window_size = 10;
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
        Scatter::new(
            inference_timestamps[window_size..].to_vec(),
            sliding_ratios.clone(),
        )
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
                    .range(vec![0, experiment_time]),
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
                    .range(vec![0, experiment_time]),
            )
            .y_axis(PAxis::new().title(Title::new("MSE of Estimated FoC"))),
    );
    plot.write_html("output/mse.html");

    plot = Plot::new();
    plot.add_trace(
        Scatter::new(
            inference_timestamps[window_size..].to_vec(),
            sliding_times.clone(),
        )
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
                    .range(vec![0, experiment_time]),
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

    circuits[0]
        .lock()
        .unwrap()
        .to_svg("output/final_rc.svg")
        .unwrap();
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
