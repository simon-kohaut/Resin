pub use crate::circuit::compile::{compile, Args};
pub use crate::circuit::leaf::{shared_leaf, Leaf, SharedLeaf};
pub use crate::circuit::model::{Model, SharedModel};
pub use crate::circuit::reactive_circuit::{add_model, ReactiveCircuit, SharedReactiveCircuit};

pub mod compile;
pub mod leaf;
pub mod model;
pub mod morphisms;
pub mod reactive_circuit;

#[macro_export]
macro_rules! lift {
    ($circuit:expr, $($leaf:expr),+) =>
    {
        // Lift each individual leaf node
        $(
            $crate::circuit::morphisms::lift_leaf($circuit, $leaf);
        )*

        // Prune the resulting new circuit
        $crate::circuit::morphisms::prune(Some($circuit.clone()));
    }
}

#[macro_export]
macro_rules! drop {
    ($circuit:expr, $($leaf:expr),+) =>
    {
        // Drop each individual leaf node
        $(
            $crate::circuit::morphisms::drop_leaf($circuit, $leaf);
        )*

        // Prune the resulting new circuit
        $crate::circuit::morphisms::prune(Some($circuit.clone()));
    }
}
