use super::{Leaf, SharedLeaf};

pub struct Category {
    pub name: String,
    pub leafs: Vec<SharedLeaf>,
}

impl Category {
    pub fn new(name: &str) -> Self {
        let positive = Leaf::new(&0.0, &0.0, name).share();
        let negative = Leaf::new(&1.0, &0.0, &format!("¬{}", name)).share();

        Self {
            name: name.to_owned(),
            leafs: vec![positive, negative],
        }
    }
}
