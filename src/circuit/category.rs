use super::Leaf;

pub struct Category {
    pub name: String,
    pub leafs: Vec<Leaf>,
}

impl Category {
    pub fn new(name: &str, value: f64) -> Self {
        let positive = Leaf::new(value, 0.0, name);
        let negative = Leaf::new(1.0 - value, 0.0, &format!("-{}", name));

        Self {
            name: name.to_owned(),
            leafs: vec![positive, negative],
        }
    }
}
