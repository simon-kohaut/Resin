pub use crate::circuit::add::Add;
pub use crate::circuit::compile::{compile, Args};
pub use crate::circuit::leaf::Leaf;
pub use crate::circuit::mul::Mul;
pub use crate::circuit::reactive::ReactiveCircuit;
pub use crate::circuit::rc::RC;

pub mod add;
pub mod category;
pub mod compile;
pub mod ipc;
pub mod leaf;
pub mod memory;
pub mod mul;
pub mod reactive;
pub mod rc;

// #[macro_export]
// macro_rules! lift {
//     ($lifted_circuit:expr, $circuit:expr, $leaf:expr) => {
//         // Lift each individual leaf node
//         if $circuit.lock().unwrap().contains($leaf) {
//             println!("Construct root");
//             let root = ReactiveCircuit::empty_new().share();
//             println!("Construct model");
//             let _ = Model::new(&vec![], &Some($circuit.clone()), &Some(root.clone()));
//             println!("Lift root");
//             $crate::circuit::morphisms::lift_leaf(&root, $leaf);
//         } else {
//             let circuits = $crate::circuit::morphisms::search_leaf_ahead($circuit, $leaf);
//             $crate::circuit::morphisms::parallel_lift_leaf(circuits, $leaf);
//         }

//         // Prune the resulting new circuit
//         $lifted_circuit = $crate::circuit::morphisms::prune(Some(
//             crate::circuit::reactive_circuit::get_root(&$circuit),
//         ))
//         .unwrap();
//     };
// }

// #[macro_export]
// macro_rules! drop {
//     ($dropped_circuit:expr, $circuit:expr, $leaf:expr) => {
//         // Drop each individual leaf node
//         let circuits = $crate::circuit::morphisms::search_leaf($circuit, $leaf);
//         $crate::circuit::morphisms::parallel_drop_leaf(circuits, $leaf);

//         // Prune the resulting new circuit
//         $dropped_circuit = $crate::circuit::morphisms::prune(Some($circuit.clone())).unwrap();
//     };
// }
