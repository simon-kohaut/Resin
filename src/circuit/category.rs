use super::semiring::{LogProb, Semiring};
use super::{leaf::Leaf, Vector};

/// A complementary leaf pair representing a probabilistic atom.
///
/// `leafs[0]` holds the positive probability `p`; `leafs[1]` holds `1 − p`.
/// Negation is computed via `S::negate` in the semiring's encoded space, so
/// `Category` works correctly under any `Semiring` implementation.
pub struct Category<S: Semiring = LogProb> {
    pub name: String,
    pub leafs: Vec<Leaf<S>>,
}

impl<S: Semiring> Category<S> {
    /// Creates a positive leaf named `name` with value `p` and a negative leaf
    /// named `"-name"` with value `S::negate(S::encode(p))`.
    pub fn new(name: &str, value: Vector) -> Self {
        // These leaves are temporary: their encoded values are extracted via
        // get_value() and re-encoded by create_leaf with the real circuit index.
        // Placeholder leaf_index = 0 is safe here.
        let positive = Leaf::<S>::new(value.clone(), 0.0, name, 0);
        let neg_encoded: Vector = value.mapv(|p| S::negate(S::encode(p))).into_shared();
        let negative = Leaf::<S>::new_encoded(neg_encoded, 0.0, &format!("-{}", name), 1);

        Self {
            name: name.to_owned(),
            leafs: vec![positive, negative],
        }
    }
}
