mod kalman;
mod model;

pub use crate::tracking::kalman::Kalman;
pub use crate::tracking::model::LinearModel;

use ndarray::{Array1, Array2};

pub type Vector = Array1<f64>;
pub type Matrix = Array2<f64>;
