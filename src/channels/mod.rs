pub mod clustering;
pub mod estimator;
pub mod generators;
pub mod ipc;
pub mod manager;

pub use crate::channels::estimator::FoCEstimator;

use ndarray::{ArcArray1, ArcArray2};

pub type Vector = ArcArray1<f64>;
pub type Matrix = ArcArray2<f64>;
