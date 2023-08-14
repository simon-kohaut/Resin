#![allow(dead_code)]

mod frequency;
mod kalman;
mod nodes;
mod reactive_circuit;
mod utility;

use crate::nodes::shared_leaf;
use crate::reactive_circuit::{Model, ReactiveCircuit};


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

    println!("Original: \t\t{} \t\t= {}", &rc, rc.value());
    rc = rc.lift(vec![a.clone()]);
    println!("Lift {{a}}: \t\t{} \t\t= {}", &rc, rc.value());

    rc = rc.drop(vec![a.clone()]);
    println!("Drop {{a}}: \t\t{} \t\t= {}", &rc, rc.value());

    rc = rc.lift(vec![c.clone()]);
    println!("Lift {{c}}: \t\t{} \t= {}", &rc, rc.value());

    rc = rc.drop(vec![c.clone()]);
    println!("Drop {{c}}: \t\t{} \t\t= {}", &rc, rc.value());

    rc = rc.drop(vec![a.clone(), b.clone(), c.clone()]);
    println!("Drop {{a, b, d}}: \t{} \t= {}", &rc, rc.value());

    rc = rc.drop(vec![e.clone(), f.clone()]);
    println!("Drop {{e, f}}: \t\t{} \t= {}", &rc, rc.value());

    rc = rc.lift(vec![a.clone(), b.clone(), c.clone()]);
    println!("Lift {{a, b, d}}: \t{} \t= {}", &rc, rc.value());

    rc = rc.lift(vec![e.clone(), f.clone()]);
    println!("Lift {{e, f}}: \t\t{} \t\t= {}", &rc, rc.value());

    rc = rc.drop(vec![a.clone(), b.clone(), c.clone()]);
    println!("Drop {{a, b, c}}: \t{} \t= {}", &rc, rc.value());

    rc = rc.drop(vec![d.clone(), e.clone(), f.clone()]);
    println!("Drop {{d, e, f}}: \t{} \t\t= {}", &rc, rc.value());

    rc = rc.drop(vec![b.clone(), c.clone(), d.clone(), e.clone(), f.clone()]);
    println!("Drop {{b, c, d, e, f}}: \t{} \t= {}", &rc, rc.value());

    rc = rc.lift(vec![b.clone(), c.clone(), d.clone(), e.clone(), f.clone()]);
    println!("Lift {{b, c, d, e, f}}: \t{} \t\t= {}", &rc, rc.value());
}
