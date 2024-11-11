// pub use crate::circuit::leaf::{update, Foliage, Leaf};
// pub use crate::circuit::reactive::ReactiveCircuit;

pub mod algebraic;
pub mod category;
pub mod leaf;
pub mod reactive;

use ndarray::{ArcArray1, ArcArray2};

pub type Vector = ArcArray1<f64>;
pub type Matrix = ArcArray2<f64>;
