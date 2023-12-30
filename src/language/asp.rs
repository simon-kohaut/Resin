use clingo::{control, Part, ShowType, SolveMode};

use crate::language::Dnf;

pub fn solve(asp: &str) -> Dnf {
    // Setup Clingo solver
    let mut clingo_control =
        control(vec!["--models=0".to_string()]).expect("Failed creating Clingo control.");
    clingo_control
        .add("base", &[], asp)
        .expect("Failed to add ASP to Clingo.");

    // Ground the program
    let part = Part::new("base", vec![]).unwrap();
    clingo_control
        .ground(&vec![part])
        .expect("Failed to ground the ASP with Clingo.");

    // Get the solver handle
    let mut handle = clingo_control
        .solve(SolveMode::YIELD, &[])
        .expect("Failed retrieving solve handle from Clingo control.");

    // Loop over all models and build DNF Dnf
    let mut formula = Dnf::new();
    loop {
        handle.resume().expect("Failed resume on solve handle.");
        match handle.model() {
            Ok(Some(stable_model)) => {
                // Get model symbols
                let atoms = stable_model
                    .symbols(ShowType::ATOMS)
                    .expect("Failed to retrieve positive symbols in the model.");

                let complement = stable_model
                    .symbols(ShowType::COMPLEMENT | ShowType::ALL)
                    .expect("Failed to retrieve complementary symbols in the model.");

                let mut clause = vec![];

                for symbol in &atoms {
                    clause.push(format!("{}", symbol));
                }

                for symbol in &complement {
                    clause.push(Dnf::negate(&format!("{}", symbol)));
                }

                formula.add_clause(clause);
            }
            Ok(None) => {
                break;
            }
            Err(e) => {
                panic!("Error: {}", e);
            }
        }
    }

    // close the solve handle
    handle.close().expect("Failed to close solve handle.");

    formula
}

#[cfg(test)]
mod tests {
    use super::solve;

    #[test]
    pub fn test_asp_solver() {
        let asp = "
        % Example taken from https://potassco.org/clingo/run/
        % Knowledge
        motive(harry).
        motive(sally).
        guilty(harry).
        
        % Rules
        innocent(Suspect) :- motive(Suspect), not guilty(Suspect).
        ";

        let formula = solve(asp);

        assert_eq!(formula.clauses.len(), 1);
        assert_eq!(
            formula.clauses,
            vec![vec![
                "motive(harry)".to_string(),
                "motive(sally)".to_string(),
                "guilty(harry)".to_string(),
                "innocent(sally)".to_string()
            ]]
        )
    }
}
