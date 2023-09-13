#![allow(dead_code)]

mod circuit;
mod frequencies;
mod language;
mod utility;

use crate::circuit::{compile, Args};
use clap::Parser;
use frequencies::{Kalman, LinearModel};
use ndarray::array;
use std::{fs::read_to_string, process::Output};

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

    Ok(())

    // let args = Args::parse();

    // let model = read_to_string(args.source).unwrap();
    // compile(model);

    // Ok(())
}
