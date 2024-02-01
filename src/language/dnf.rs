#[derive(Clone)]
pub struct Dnf {
    pub clauses: Vec<Vec<String>>,
}

impl Dnf {
    pub fn new() -> Self {
        Dnf { clauses: vec![] }
    }

    pub fn add_clause(&mut self, clause: Vec<String>) {
        self.clauses.push(clause);
    }

    pub fn remove(&mut self, variable: &str) {
        for clause in &mut self.clauses {
            clause.retain(|l| Dnf::get_variable(l) != variable);
        }
    }

    pub fn is_negated(literal: &str) -> bool {
        literal.chars().nth(0).expect("Literal had empty name!") == '-'
    }

    pub fn negate(literal: &str) -> String {
        if Dnf::is_negated(literal) {
            literal[1..].to_owned()
        } else {
            format!("-{literal}")
        }
    }

    pub fn get_variable(literal: &str) -> String {
        if Dnf::is_negated(literal) {
            literal[1..].to_owned()
        } else {
            literal.to_owned()
        }
    }
}
