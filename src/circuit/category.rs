use super::Leaf;

pub struct Category {
    pub name: String,
    pub leafs: Vec<Leaf>,
}

impl Category {
    pub fn new(name: &str) -> Self {
        let positive = Leaf::new(&0.0, &0.0, name);
        let negative = Leaf::new(&1.0, &0.0, &format!("¬{}", name));

        Self {
            name: name.to_owned(),
            leafs: vec![positive, negative],
        }
    }
}
