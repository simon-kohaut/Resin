mod asp;
mod concepts;
mod dnf;
mod matching;
mod resin;

pub use crate::language::concepts::{Clause, ResinType, Source, Target};
pub use crate::language::dnf::Dnf;
pub use crate::language::resin::Resin;

use ndarray::{ArcArray1, ArcArray2};

pub type Vector = ArcArray1<f64>;
pub type Matrix = ArcArray2<f64>;
