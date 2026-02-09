use std::str::FromStr;
use std::sync::{Arc, Mutex};

use super::Vector;
use super::{Clause, Source, Target};
use crate::channels::manager::Manager;
use crate::circuit::category::Category;
use crate::language::{asp::solve, Dnf};

pub type SharedStorage = Arc<Mutex<Vec<f64>>>;

pub struct Resin {
    pub clauses: Vec<Clause>,
    pub sources: Vec<Source>,
    pub targets: Vec<Target>,
    pub manager: Manager,
    value_size: usize,
}

impl Resin {
    pub fn compile(model: &str, value_size: usize, verbose: bool) -> Result<Resin, Box<dyn std::error::Error>> {
        // Parse and setup Resin runtime environment
        let mut resin: Resin = model.parse().unwrap();
        resin.value_size = value_size;
        resin.manager.reactive_circuit.lock().unwrap().value_size = value_size;

        if verbose {
            println!("Compiling Resin from program:");
            println!("{}", model);
        }

        // Setup data distribution through signal leafs
        resin.value_size = value_size;
        resin.setup_signals()?;
        if verbose {
            println!("Setup {} signals.", resin.manager.reactive_circuit.lock().unwrap().leafs.len());
        }

        // Pass data to Clingo and obtain stable models
        for target_index in 0..resin.targets.len() {
            // Compile Resin into ASP
            let program = resin.to_asp(target_index);
            if verbose {
                println!(
                    "Compiled Resin for target {} into ASP:",
                    resin.targets[target_index].name
                );
                println!("{}", program);
            }

            // Solve ASP and obtain DNF formula from which the target is removed
            let mut dnf = solve(&program, verbose);
            dnf.remove(&resin.targets[target_index].name);

            if verbose {
                println!(
                    "Solved Resin into a DNF with {} clauses",
                    dnf.clauses.len()
                );
            }

            // Build the RC from the DNF
            resin.circuit_from_dnf(dnf, &resin.targets[target_index].name);

            // TODO: Handle multiple targets
            break;
        }

        // Return the compiled Resin program
        Ok(resin)
    }

    pub fn to_asp(&self, target_index: usize) -> String {
        let mut asp = "".to_string();

        for source in &self.sources {
            asp.push_str(&source.to_asp());
        }

        for clause in &self.clauses {
            asp.push_str(&clause.to_asp());
        }

        asp.push_str(&self.targets[target_index].to_asp());
        asp
    }

    pub fn setup_signals(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        // Create all source channels and parameter leafs
        for source in &self.sources {
            let index_normal = self
                .manager
                .create_leaf(&source.name, Vector::zeros(self.value_size), 0.0);

            let index_inverted = self.manager.create_leaf(
                &format!("-{}", source.name),
                Vector::ones(self.value_size),
                0.0,
            );
            self.manager.read_dual(index_normal, index_inverted, &source.channel)?;
        }

        for clause in &self.clauses {
            // Clauses that are deterministic do not need to be included in model
            if clause.probability.is_none() {
                continue;
            }

            let category = Category::new(
                &clause.head,
                clause.probability.unwrap() * Vector::ones(self.value_size),
            );

            self.manager
                .create_leaf(&category.leafs[0].name, category.leafs[0].get_value(), 0.0);
            self.manager
                .create_leaf(&category.leafs[1].name, category.leafs[1].get_value(), 0.0);
        }

        Ok(())
    }

    pub fn circuit_from_dnf(&self, dnf: Dnf, target_token: &str) {
        // Get indexing from name to foliage
        let index_map = self.manager.get_index_map();

        // A DNF is an OR over AND, i.e., a sum over products without further hirarchy
        let mut sum_product = Vec::new();
        for clause in &dnf.clauses {
            let mut product = vec![];

            for literal in clause {
                product.push(index_map[literal] as u32);
            }

            sum_product.push(product);
        }

        // Add the target to the ReactiveCircuit
        self.manager.reactive_circuit.lock().unwrap().add_sum_product(&sum_product, target_token);
    }
}

impl FromStr for Resin {
    type Err = Box<dyn std::error::Error>;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut resin = Resin {
            clauses: vec![],
            sources: vec![],
            targets: vec![],
            manager: Manager::new(1),
            value_size: 1,
        };

        // Parse Resin source line by line into appropriate data structures
        for line in input.lines() {
            let source = line.parse::<Source>();
            if source.is_ok() {
                resin.sources.push(source.unwrap());
                continue;
            }

            let target = line.parse::<Target>();
            if target.is_ok() {
                resin.targets.push(target.unwrap());
                continue;
            }

            let clause = line.parse::<Clause>();
            if clause.is_ok() {
                resin.clauses.push(clause.unwrap());
                continue;
            }
        }

        Ok(resin)
    }
}

#[cfg(test)]
mod tests {

    use std::{collections::HashMap, fmt::Debug};
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::path::Path;
    use std::time::Instant;
    
    use polars::io::mmap::MmapBytesReader;
    use polars::prelude::*;

    use crate::channels::clustering::partitioning;

    use super::*;

    #[test]
    fn test_clauses() {
        let code = "test.";
        let clause: Clause = code.parse().expect("Parse clause failed!");
        assert!(clause.body.is_empty());
        assert_eq!(clause.code, code);
        assert_eq!(clause.head, "test");
        assert!(clause.probability.is_none());

        let code = "pilot(ben).";
        let clause: Clause = code.parse().expect("Parse clause failed!");
        assert!(clause.body.is_empty());
        assert_eq!(clause.code, code);
        assert_eq!(clause.head, "pilot(ben)");
        assert!(clause.probability.is_none());

        let code = "heavy(drone_1) <- P(0.8).";
        let clause: Clause = code.parse().expect("Parse clause failed!");
        assert!(clause.body.is_empty());
        assert_eq!(clause.code, code);
        assert_eq!(clause.head, "heavy(drone_1)");
        assert_eq!(clause.probability.unwrap(), 0.8);

        let code =
            "unsafe(drone_1, drone_2) <- P(0.65) if close(drone_1, drone_2) and heavy(drone_1).";
        let clause: Clause = code.parse().expect("Parse clause failed!");
        assert_eq!(clause.code, code);
        assert_eq!(clause.head, "unsafe(drone_1, drone_2)");
        assert_eq!(clause.probability.unwrap(), 0.65);
        assert_eq!(
            clause.body,
            vec!["close(drone_1, drone_2)", "heavy(drone_1)"]
        );
    }

    #[test]
    fn test_resin_model() {
        let model = "
        close(a,b) <- P(0.8).
        close(a,c) <- P(0.7).

        unsafe if close(X,Y).

        unsafe -> target(\"/safety\").
        ";

        // Compile Resin runtime environment
        let resin = Resin::compile(model, 1, true);
        assert!(resin.is_ok());
        let resin = resin.unwrap();

        // Show circuit
        let _ = resin.manager.reactive_circuit.lock().unwrap().to_combined_svg("output/test/test_resin_model_circuits.svg");

        println!("{:#?}", resin.manager.reactive_circuit.lock().unwrap().targets);

        // Count the correct number of Resin elements
        assert_eq!(resin.clauses.len(), 3);
        assert_eq!(resin.sources.len(), 0);
        assert_eq!(resin.targets.len(), 1);

        // Check a correct result for target signal
        let result = resin.manager.reactive_circuit.lock().unwrap().update();
        assert_eq!(result["unsafe"], Vector::from(vec![0.94]));
    }

}
