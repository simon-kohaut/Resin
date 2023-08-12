#![allow(dead_code)]

mod frequency;
mod kalman;
mod nodes;
mod reactive_circuit;
mod utility;

use crate::nodes::shared_leaf;
use crate::reactive_circuit::{drop, lift, prune, Model, ReactiveCircuit};


fn main() {
    let a = shared_leaf(0.5, 0.0, "a".to_string());
    let b = shared_leaf(0.9, 0.0, "b".to_string());
    let c = shared_leaf(0.25, 0.0, "c".to_string());
    let d = shared_leaf(0.3, 0.0, "d".to_string());
    let e = shared_leaf(0.8, 0.0, "e".to_string());
    let f = shared_leaf(0.9, 0.0, "f".to_string());

    let mut rc = ReactiveCircuit::new();
    rc.add_model(Model::new(
        vec![a.clone(), b.clone(), d.clone(), e.clone()],
        None,
    ));
    rc.add_model(Model::new(
        vec![a.clone(), c.clone(), e.clone(), f.clone()],
        None,
    ));

    println!("Original: {}", &rc);
    println!("Value: {}", rc.value());
    rc = drop(&rc, a.clone());
    rc = drop(&rc, b.clone());
    rc = drop(&rc, c.clone());
    rc = drop(&rc, d.clone());
    rc = drop(&rc, f.clone());
    println!("Drop {{a, b, c, d, f}}: {}", &rc);
    println!("Value: {}", rc.value());
    rc = drop(&rc, e.clone());
    rc = drop(&rc, f.clone());
    println!("Drop {{e, f}}: {}", &rc);
    println!("Value: {}", rc.value());
    rc = lift(&rc, a.clone());
    rc = lift(&rc, b.clone());
    rc = lift(&rc, d.clone());
    rc = lift(&rc, e.clone());
    rc = lift(&rc, f.clone());
    println!("Lift {{a, b, d, e, f}}: {}", &rc);
    println!("Value: {}", rc.value());
    rc = prune(&rc).unwrap();
    println!("Prune: {}", &rc);
    println!("Value: {}", rc.value());
}
