#![allow(dead_code)]

// use plotly::{Plot, Scatter};
use std::sync::{Arc, Mutex};
use std_msgs::msg::String as StringMsg;

mod frequency;
mod kalman;
mod nodes;
mod reactive_circuit;
mod utility;

use crate::nodes::shared_leaf;
use crate::reactive_circuit::{drop, Model, ReactiveCircuit};
use crate::utility::power_set;

fn main() {
    // Result<(), rclrs::RclrsError> {
    // let mut plot = Plot::new();

    // let xs = vec![0, 1, 2, 3, 4, 5, 6, 7, 8];
    // let ys = vec![3062, 587, 284, 103, 33, 4, 2];

    // let trace = Scatter::new(xs, ys);
    // plot.add_trace(trace);

    // plot.write_html("out.html");

    // let mut state = array![0.0, 1.0];
    // let mut measurement = array![0.0];
    // let input = array![0.0, 0.0];
    // let forward_model = array![[1.0, 1.0], [0.0, 1.0],];
    // let input_model = array![[0.0, 0.0], [0.0, 1.0]];
    // let output_model = array![[1.0, 0.0],];

    // let model = kalman::LinearModel::new(forward_model, input_model, output_model);

    // for _i in 0..10 {
    //     state = model.forward(&state, &input);
    //     println!("{}", state);
    //     measurement = model.measure(&state);
    //     println!("{}", measurement);
    // }

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
    rc = drop(&rc, a.clone());
    rc = drop(&rc, b.clone());
    rc = drop(&rc, d.clone());
    println!("Drop {{a, b, d}}: {}", &rc);
    rc = drop(&rc, e.clone());
    rc = drop(&rc, f.clone());
    println!("Drop {{e, f}}: {}", &rc);
    println!("{}", rc.value());

    // println!("Lift a: \n {:#?}", rc);
    // drop(rc.clone(), a.clone());
    // println!("Drop a: \n {:#?}", rc);
    // println!("{:#?}", rc);
    // println!("{}", rc.lock().unwrap().value());
    // lift(rc.clone(), a.clone());
    // println!("{:#?}", rc);

    // let mut rc = RC::new();
    // rc.add_product(vec![a.clone(), b.clone()]);
    // rc.add_product(vec![a.clone(), c.clone()]);

    // println!("{}", rc.value());

    // let all = vec![a, b, c];
    // let power_set = power_set(&all);

    // let c = LeafNode::new(2.0);

    // let mut tmp1: Vec<Box<dyn Signal>> = vec![Box::new(a), Box::new(b)];
    // let mut sum_node = SumNode::new(&mut tmp1, array![1.0, 1.0]);

    // let mut tmp2: Vec<Box<dyn Signal>> = vec![sum_node, c];
    // let mut product_node = ProductNode::new(&mut tmp2);

    // let mut max_node = MaxNode::new(&mut tmp3, array![1.0, 1.0]);

    // sum_node.update();
    // product_node.update();
    // max_node.update();
    // println!("Sum of a and b: {}", sum_node.get_value());
    // println!("Product of a and b: {}", product_node.get_value());
    // println!("Max of a and b: {}", max_node.get_value());
}
