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

    // pub fn write_dimacs(&self) -> String {
    //     // Create DIMACS file that represents CNF for various compilers
    //     let mut dimacs = "".to_string();
    //     let cnf = self.to(FormulaType::CNF);

    //     // Overall number of clauses
    //     let number_clauses = cnf.clauses.len();

    //     // Assign a number to each atom
    //     // First flatten the clauses into a single list of literals
    //     let literals = cnf
    //         .clauses
    //         .iter()
    //         .cloned()
    //         .fold(vec![], |mut acc, mut clause| {
    //             acc.append(&mut clause);
    //             acc
    //         });
    //     // Seond, build HashMap<Str, i32> to map from variable to number
    //     let mut index_map = HashMap::new();
    //     for literal in literals {
    //         let variable = Formula::get_variable(&literal);

    //         if !index_map.contains_key(&variable) {
    //             index_map.insert(variable, index_map.len() + 1);
    //         }
    //     }

    //     // The number of variables is the number of entries in the HashMap
    //     let number_variables = index_map.len();

    //     // Set DIMACS header
    //     dimacs += &format!("p cnf {} {}\n", number_variables, number_clauses);
    //     for clause in &cnf.clauses {
    //         let mut dimacs_clause = clause.iter().fold("".to_string(), |acc, literal| {
    //             let index = index_map.get(&Formula::get_variable(literal)).unwrap();

    //             if Formula::is_negated(literal) {
    //                 acc + &format!("-{} ", index)
    //             } else {
    //                 acc + &format!("{} ", index)
    //             }
    //         });
    //         dimacs_clause += "0\n";

    //         dimacs += &dimacs_clause;
    //     }

    //     dimacs
    // }
}
