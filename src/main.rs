#![allow(dead_code)]

mod circuit;
mod frequencies;
mod language;
mod utility;

// use crate::circuit::{compile, Args};
// use clap::Parser;
use frequencies::{Kalman, LinearModel};
// use linfa::prelude::Records;
use ndarray::{array, Array2, concatenate, Axis};
// use std::{fs::read_to_string, process::Output};

use linfa::traits::Transformer;
use linfa::{Dataset};
use linfa_clustering::{Dbscan};
use linfa_datasets::iris;
use plotly::{Plot, Scatter};
use plotly::common::Mode;

fn create_data() -> Array2<f64> {
    let mut data = vec![];
    for i in 0..100 {
        data.push(1.0 * i as f64);
    }
    return Array2::from_shape_vec((100, 1), data).unwrap();
}

fn linfa_example() {
    // Let's generate a synthetic dataset: three blobs of observations
    // (100 points each) centered around our `expected_centroids`
    let my_observations = Dataset::new(
        create_data(), 
        array![0.0]
    );

    // Let's configure and run our DBSCAN algorithm
    // We use the builder pattern to specify the hyperparameters
    // `min_points` is the only mandatory parameter.
    // If you don't specify the others (e.g. `tolerance`)
    // default values will be used.
    // println!("{:?}", observations);
    let min_points = 3;
    // let dataset = Dataset::new(my_observations.into(), targets.into());
    let clusters = Dbscan::params(min_points)
        .tolerance(1.1)
        .transform(my_observations);

    let mut c0_xs = vec![];
    let mut c1_xs = vec![];
    let mut c0_ys = vec![];
    let mut c1_ys = vec![];

    println!("{:?}", clusters.targets);

    for i in 0..clusters.records.shape()[0] - 1 {
        if clusters.targets[i].is_none() {
            continue;
        }

        if clusters.targets[i].unwrap() == 0 {
            c0_xs.push(clusters.records[(i, 0)]);
            // c0_ys.push(clusters.records[(i, 1)]);
            c0_ys.push(0.0);
        } else {
            c1_xs.push(clusters.records[(i, 0)]);
            // c1_ys.push(clusters.records[(i, 1)]);
            c1_ys.push(1.0);
        }

    }

    let mut plot = Plot::new();
    let c0_trace = Scatter::new(c0_xs, c0_ys).mode(Mode::Markers);
    let c1_trace = Scatter::new(c1_xs, c1_ys).mode(Mode::Markers);
    plot.add_trace(c0_trace);
    plot.add_trace(c1_trace);

    plot.write_html("out.html");
}

fn main() -> std::io::Result<()> {
    let forward_model = array![[1.0, 1.0], [0.0, 1.0]];
    let input_model = array![[0.0, 0.0]];
    let output_model = array![[1.0, 0.0]];
    let prediction = array![0.0, 0.0];
    let prediction_covariance = array![[1.0, 0.0], [0.0, 1.0]];
    let process_noise = array![[1.0, 0.0], [0.0, 1.0]];
    let sensor_noise = array![[1.0]];
    let input = array![0.0, 0.0];

    let model = LinearModel::new(&forward_model, &input_model, &output_model);
    let mut kalman = Kalman::new(
        &prediction,
        &prediction_covariance,
        &process_noise,
        &sensor_noise,
        &model,
    );

    for i in 0..100 {
        kalman.predict(&input);
        kalman.update(&array![0.5 + i as f64]);
        println!("{}", i);
        println!("{}", kalman.estimate);
        println!("{}", kalman.estimate_covariance);
        println!("");
    }

    linfa_example();

    Ok(())

    // let args = Args::parse();

    // let model = read_to_string(args.source).unwrap();
    // compile(model);

    // Ok(())
}
