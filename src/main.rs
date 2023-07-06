#![allow(dead_code)]

// use plotly::{Plot, Scatter};
use std::sync::{Arc, Mutex};
use std_msgs::msg::String as StringMsg;

mod frequency;
mod kalman;
mod nodes;
mod reactive_circuit;

use crate::nodes::shared_leaf;
use crate::reactive_circuit::ReactiveCircuit;

struct RepublisherNode {
    node: rclrs::Node,
    _subscription: Arc<rclrs::Subscription<StringMsg>>,
    publisher: rclrs::Publisher<StringMsg>,
    data: Arc<Mutex<Option<StringMsg>>>, // (2)
}

impl RepublisherNode {
    fn new(context: &rclrs::Context) -> Result<Self, rclrs::RclrsError> {
        let mut node = rclrs::Node::new(context, "republisher")?;
        let data = Arc::new(Mutex::new(None)); // (3)
        let data_cb = Arc::clone(&data);
        let _subscription = {
            // Create a new shared pointer instance that will be owned by the closure
            node.create_subscription(
                "in_topic",
                rclrs::QOS_PROFILE_DEFAULT,
                move |msg: StringMsg| {
                    // This subscription now owns the data_cb variable
                    *data_cb.lock().unwrap() = Some(msg); // (4)
                },
            )?
        };

        let publisher = node.create_publisher("out_topic", rclrs::QOS_PROFILE_DEFAULT)?;
        Ok(Self {
            node,
            _subscription,
            publisher,
            data,
        })
    }

    fn republish(&self) -> Result<(), rclrs::RclrsError> {
        if let Some(s) = &*self.data.lock().unwrap() {
            self.publisher.publish(s)?;
        }
        Ok(())
    }
}

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
    let c = shared_leaf(0.1, 0.0, "c".to_string());

    let rc =
        ReactiveCircuit::from_worlds(vec![vec![a.clone(), b.clone()], vec![a.clone(), c.clone()]]);
    println!("{}", rc.value());
    rc.remove(&a);
    println!("{}", rc.value());

    let all = vec![a, b, c];
    let power_set = ReactiveCircuit::power_set(&all);
    for set in power_set {
        println!(
            "{}",
            set.iter().fold(String::new(), |acc, &leaf| acc
                + &leaf.lock().unwrap().to_string()
                + ", ")
        );
    }

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

    // let context = rclrs::Context::new(std::env::args())?;
    // let republisher = Arc::new(RepublisherNode::new(&context)?);
    // let republisher_other_thread = Arc::clone(&republisher);
    // std::thread::spawn(move || -> Result<(), rclrs::RclrsError> {
    //     loop {
    //         use std::time::Duration;
    //         std::thread::sleep(Duration::from_millis(1000));
    //         republisher_other_thread.republish()?;
    //     }
    // });
    // rclrs::spin(&republisher.node)
}
