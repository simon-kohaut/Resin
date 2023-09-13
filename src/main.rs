#![allow(dead_code)]

mod circuit;
mod frequency;
mod kalman;
mod language;
mod utility;

use crate::circuit::{compile, Args};
use clap::Parser;
use std::fs::read_to_string;

fn main() -> std::io::Result<()> {
    let args = Args::parse();

    let model = read_to_string(args.source).unwrap();
    compile(model);

    Ok(())
}
