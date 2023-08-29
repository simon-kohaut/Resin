#![allow(dead_code)]

mod frequency;
mod kalman;
mod nodes;
mod reactive_circuit;
mod utility;
mod resin;

use crate::nodes::shared_leaf;
use crate::reactive_circuit::{Model, ReactiveCircuit};
use crate::resin::{Args, parse};
use clap::Parser;
use std::fs::read_to_string;

fn main() -> std::io::Result<()> {
    let args = Args::parse();

    let model = read_to_string(args.source).unwrap();
    parse(model);
    return Ok(());

    let a = shared_leaf(0.5, 0.0, "a".to_string());
    let b = shared_leaf(0.9, 0.0, "b".to_string());
    let c = shared_leaf(0.25, 0.0, "c".to_string());
    let d = shared_leaf(0.3, 0.0, "d".to_string());
    let e = shared_leaf(0.8, 0.0, "e".to_string());
    let f = shared_leaf(0.9, 0.0, "f".to_string());
    let g = shared_leaf(0.9, 0.0, "g".to_string());
    let h = shared_leaf(0.9, 0.0, "h".to_string());

    let mut rc = ReactiveCircuit::new();
    rc.add_model(Model::new(
        vec![a.clone(), b.clone(), d.clone(), e.clone()],
        None,
    ));
    rc.add_model(Model::new(
        vec![a.clone(), c.clone(), e.clone(), f.clone()],
        None,
    ));
    rc.add_model(Model::new(vec![c.clone(), f.clone()], None));
    rc.add_model(Model::new(vec![g.clone(), h.clone()], None));

    println!("Original: \t\t{} \t\t= {}", &rc, rc.value());
    rc.to_svg("output/0".to_string())?;

    rc = rc.lift(vec![b.clone()]);
    println!("Changed circuit: \t{} \t\t= {}", &rc, rc.value(),);
    rc.to_svg("output/1".to_string())?;

    rc = rc.lift(vec![a.clone()]);
    println!("Changed circuit: \t{} \t\t= {}", &rc, rc.value(),);
    rc.to_svg("output/2".to_string())?;

    rc = rc.drop(vec![d.clone()]);
    println!("Changed circuit: \t{} \t\t= {}", &rc, rc.value(),);
    rc.to_svg("output/3".to_string())?;

    rc = rc.drop(vec![e.clone()]);
    println!("Changed circuit: \t{} \t\t= {}", &rc, rc.value(),);
    rc.to_svg("output/4".to_string())?;

    rc = rc.drop(vec![b.clone()]);
    println!("Changed circuit: \t{} \t\t= {}", &rc, rc.value(),);
    rc.to_svg("output/5".to_string())?;
    
    rc = rc.drop(vec![a.clone()]);
    println!("Changed circuit: \t{} \t\t= {}", &rc, rc.value(),);
    rc.to_svg("output/6".to_string())?;

    rc = rc.lift(vec![d.clone()]);
    println!("Changed circuit: \t{} \t\t= {}", &rc, rc.value(),);
    rc.to_svg("output/7".to_string())?;

    rc = rc.lift(vec![e.clone()]);
    println!("Changed circuit: \t{} \t\t= {}", &rc, rc.value(),);
    rc.to_svg("output/8".to_string())?;

    rc = rc.lift(vec![e.clone()]);
    println!("Changed circuit: \t{} \t\t= {}", &rc, rc.value(),);
    rc.to_svg("output/9".to_string())?;

    Ok(())
}
