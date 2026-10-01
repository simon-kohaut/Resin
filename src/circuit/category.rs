use super::semiring::{LogProb, Semiring};
use super::{leaf::Leaf, Vector};

/// A complementary leaf pair representing a probabilistic atom.
///
/// `leafs[0]` holds the positive probability `p`; `leafs[1]` holds `1 − p`.
/// The complement is computed via `S::negate`, so `Category` works correctly
/// under any `Semiring` implementation. `values` holds both leaves' input
/// values, as written to a source.
pub struct Category<S: Semiring = LogProb> {
    pub name: String,
    pub leafs: Vec<Leaf<S>>,
    pub values: [Vector; 2],
}

impl<S: Semiring> Category<S> {
    /// Creates a positive leaf named `name` with value `p` and a negative leaf
    /// named `"-name"` with value `S::decode(S::negate(S::encode(p)))`.
    pub fn new(name: &str, value: Vector) -> Self {
        let negated: Vector = value
            .mapv(|p| S::decode(S::negate(S::encode(p))))
            .into_shared();
        let negative_name = format!("-{}", name);
        Self {
            name: name.to_owned(),
            leafs: vec![
                Leaf::<S>::new(value.clone(), 0.0, name, 0),
                Leaf::<S>::new(negated.clone(), 0.0, &negative_name, 1),
            ],
            values: [value, negated],
        }
    }
}
