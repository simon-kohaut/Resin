/// A formula in Disjunctive Normal Form (DNF): an OR over conjunctive clauses.
///
/// Each inner `Vec<String>` is one conjunction (AND) of literals.
/// Literals may be positive (`"safe"`) or negated (`"-safe"`).
/// This representation is populated directly from Clingo stable models and then
/// used to build the reactive arithmetic circuit.
#[derive(Clone)]
pub struct Dnf {
    pub clauses: Vec<Vec<String>>,
}

impl Dnf {
    /// Creates an empty DNF with no clauses.
    pub fn new() -> Self {
        Dnf { clauses: vec![] }
    }

    /// Appends a conjunctive clause to the formula.
    pub fn add_clause(&mut self, clause: Vec<String>) {
        self.clauses.push(clause);
    }

    /// Removes every occurrence of `variable` (positive or negative) from all
    /// clauses.  Used to strip the target atom before building the circuit.
    pub fn remove(&mut self, variable: &str) {
        for clause in &mut self.clauses {
            clause.retain(|l| Dnf::get_variable(l) != variable);
        }
    }

    /// Returns `true` if `literal` is negated, i.e. starts with `'-'`.
    pub fn is_negated(literal: &str) -> bool {
        literal.starts_with('-')
    }

    /// Returns the logical negation of `literal`:
    /// strips the leading `'-'` for already-negated literals,
    /// prepends `'-'` for positive ones.
    pub fn negate(literal: &str) -> String {
        if Dnf::is_negated(literal) {
            literal[1..].to_owned()
        } else {
            format!("-{literal}")
        }
    }

    /// Returns the underlying variable name of a literal, stripping any leading `'-'`.
    pub fn get_variable(literal: &str) -> String {
        if Dnf::is_negated(literal) {
            literal[1..].to_owned()
        } else {
            literal.to_owned()
        }
    }
}
