#![allow(dead_code)]

mod circuit;
mod frequencies;
mod language;
mod tracking;

use crate::circuit::ipc::{retreive_messages, RandomizedIpcChannel};
use crate::circuit::{compile, Args};
use crate::language::Resin;
use circuit::{add_model, shared_leaf, Model, ReactiveCircuit, SharedLeaf, SharedReactiveCircuit};
use clap::Parser;
use frequencies::FoCEstimator;
use itertools::Itertools;
use linfa::prelude::Records;
use linfa::traits::Transformer;
use linfa::Dataset;
use linfa_clustering::Dbscan;
use linfa_datasets::iris;
use ndarray::{array, concatenate, Array2, Axis};
use plotly::common::Mode;
use plotly::{Plot, Scatter};
use plotly::layout::{Layout, Legend, Axis as PAxis};
use plotly::common::Title;
use std::io::{stdin, stdout, Read, Write};
use std::{fs::read_to_string, process::Output};
use tracking::{Kalman, LinearModel};
use std::time::{SystemTime};

pub fn power_set<T: Clone>(leafs: &[T]) -> Vec<Vec<T>> {
    let mut power_set = Vec::new();
    for i in 0..leafs.len() + 1 {
        for set in leafs.iter().cloned().combinations(i) {
            power_set.push(set);
        }
    }
    power_set
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
        add_model(&rc, &combination, &None);
    }

    (rc, leafs)
}

fn create_data(leafs: &[SharedLeaf]) -> Array2<f64> {
    let mut data = vec![];
    for leaf in leafs {
        data.push(leaf.lock().unwrap().get_frequency());
    }
    return Array2::from_shape_vec((leafs.len(), 1), data).unwrap();
}

fn frequency_adaptation(resin: &mut Resin) {
    let mut leafs: Vec<SharedLeaf> = resin.leafs.values().cloned().collect();
    leafs.sort_by(|a, b| {
        a.lock()
            .unwrap()
            .get_frequency()
            .partial_cmp(&b.lock().unwrap().get_frequency())
            .unwrap()
    });
    let my_observations = Dataset::new(create_data(&leafs), array![0.0]);

    let clusters = Dbscan::params(2).tolerance(0.1).transform(my_observations);

    let mut cluster_counter = 0;
    let mut previous_cluster = 0;
    let mut cluster_step = 0;
    for i in 0..clusters.records.shape()[0] - 1 {
        let optional_cluster = clusters.targets[i];

        match optional_cluster {
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
            for circuit in &mut resin.circuits {
                if cluster_step > 0 {
                    for _ in 0..cluster_step {
                        drop![circuit, &leafs[i]];
                    }
                } else {
                    for _ in 0..-cluster_step {
                        lift![circuit, &leafs[i]];
                    }
                }
            }
        }
    }

    // let mut plot = Plot::new();
    // for leaf in leafs {
    //     let frequency = leaf.lock().unwrap().get_frequency();
    //     let cluster = leaf.lock().unwrap().get_cluster();
    //     let name = leaf.lock().unwrap().name.to_owned();
    //     plot.add_trace(
    //         Scatter::new(vec![frequency], vec![cluster])
    //             .name(name)
    //             .mode(Mode::Markers),
    //     );
    // }
    // plot.write_html("output/leaf_frequencies.html");
}

fn pause() {
    let mut stdout = stdout();
    stdout.write(b"Press Enter to continue...").unwrap();
    stdout.flush().unwrap();
    stdin().read(&mut [0]).unwrap();
}

fn main() -> std::io::Result<()> {
    let args = Args::parse();

    let model = read_to_string(args.source).unwrap();
    let mut resin = compile(model);

    for leaf in resin.leafs.values() {
        match &leaf.lock().unwrap().ipc_channel {
            Some(channel) => {
                use rand::Rng;
                let mut rng = rand::thread_rng();
                let new_publisher =
                    RandomizedIpcChannel::new(&channel.topic, rng.gen_range(0.1..50.0));
                new_publisher.unwrap().start();
            }
            None => (),
        }
    }

    // let (rc, leafs) = randomized_rc(10);
    // println!("Value of RC = {:?}", rc.lock().unwrap().get_value());
    // lift![&rc, &leafs[0]];
    // println!("Value of RC = {:?}", rc.lock().unwrap().get_value());

    let clock = SystemTime::now();
    let mut operations = vec![];
    let mut times = vec![];
    loop {
        retreive_messages();
        frequency_adaptation(&mut resin);
        // for leaf in &resin_leafs {
        //     print!("{}, ", leaf.lock().unwrap().get_frequency());
        // }
        let (value, n_ops) = resin.circuits[0].lock().unwrap().get_value();
        // println!("{:?}", (value, n_ops));
        operations.push(n_ops);
        // resin.circuits[0]
        //     .lock()
        //     .unwrap()
        //     .to_svg(&format!("output/{}.svg", n_ops))?;
        
        match clock.elapsed() {
            Ok(elapsed) => {
                times.push(elapsed.as_secs_f64());
                if elapsed.as_secs() > 5 { break; }
            }
            Err(e) => {
                println!("Error {e:?}");
                break;
            }
        }
    }

    let mut plot = Plot::new();
    plot.add_trace(Scatter::new(times, operations));
    plot.set_layout(Layout::new()
        .title(Title::new("Sales Data"))
        .x_axis(PAxis::new().title(Title::new("Time / s")))
        .y_axis(PAxis::new().title(Title::new("#Operations")))
    );
    plot.write_html("output/operations_curve.html");

    Ok(())

    // loop {
    //     println!("Value of RC = {:?}", resin.circuits[0].lock().unwrap().get_value());
    //     retreive_messages();
    // }

    // Ok(())
}
