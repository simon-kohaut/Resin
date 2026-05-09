use super::Leaf;

use super::Vector;

/// A complementary leaf pair representing a probabilistic atom.
///
/// `leafs[0]` holds the positive probability `p`; `leafs[1]` holds `1 − p`.
/// Used by the compiler to represent probabilistic clause heads.
pub struct Category {
    pub name: String,
    pub leafs: Vec<Leaf>,
}

impl Category {
    /// Creates a positive leaf named `name` with value `p` and a negative leaf
    /// named `"-name"` with value `1 − p`.
    pub fn new(name: &str, value: Vector) -> Self {
        let positive = Leaf::new(value.clone(), 0.0, name);
        let negative = Leaf::new(1.0 - value, 0.0, &format!("-{}", name));

        Self {
            name: name.to_owned(),
            leafs: vec![positive, negative],
        }
    }
}
