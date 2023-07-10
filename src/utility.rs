use itertools::Itertools;

pub fn power_set<T>(leafs: &[T]) -> Vec<Vec<&T>> {
    let mut power_set = Vec::new();
    for i in 0..leafs.len() + 1 {
        for set in leafs.iter().combinations(i) {
            power_set.push(set);
        }
    }
    power_set
}
