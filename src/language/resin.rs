use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use super::Vector;
use super::{Clause, Source, Target};
use crate::channels::ipc::TypedWriter;
use crate::channels::manager::Manager;
use crate::circuit::category::Category;
use crate::language::concepts::{ComparisonLiteral, ResinType};
use crate::language::{asp::solve, Dnf};

pub type SharedStorage = Arc<Mutex<Vec<f64>>>;

pub struct Resin {
    pub clauses: Vec<Clause>,
    pub sources: Vec<Source>,
    pub targets: Vec<Target>,
    pub manager: Manager,
    value_size: usize,
    /// Maps each Density/Number source atom name to its registered comparisons:
    /// `(threshold, upper_tail, canonical_leaf_name)`.
    comparison_registry: HashMap<String, Vec<(f64, bool, String)>>,
}

impl Resin {
    pub fn compile(
        model: &str,
        value_size: usize,
        verbose: bool,
    ) -> Result<Resin, Box<dyn std::error::Error>> {
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
            println!(
                "Setup {} signals.",
                resin.manager.reactive_circuit.lock().unwrap().leafs.len()
            );
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
                println!("Solved Resin into a DNF with {} clauses", dnf.clauses.len());
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
            match source.message_type {
                // Probability and Boolean sources are simple probabilistic atoms.
                ResinType::Probability | ResinType::Boolean => {
                    asp.push_str(&source.to_asp());
                }
                // Density and Number sources manifest as one choice atom per comparison.
                ResinType::Density | ResinType::Number => {
                    if let Some(comparisons) = self.comparison_registry.get(&source.name) {
                        for (_, _, canonical) in comparisons {
                            asp.push_str(&format!("{{{}}}.\n", canonical));
                        }
                    }
                }
            }
        }

        for clause in &self.clauses {
            asp.push_str(&clause.to_asp());
        }

        asp.push_str(&self.targets[target_index].to_asp());
        asp
    }

    pub fn setup_signals(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        for source in &self.sources {
            match source.message_type {
                ResinType::Probability | ResinType::Boolean => {
                    // Single leaf pair, driven directly by the writer.
                    let idx_normal = self.manager.create_leaf(
                        &source.name,
                        Vector::zeros(self.value_size),
                        0.0,
                    );
                    let idx_inverted = self.manager.create_leaf(
                        &format!("-{}", source.name),
                        Vector::ones(self.value_size),
                        0.0,
                    );
                    self.manager
                        .read_dual(idx_normal, idx_inverted, &source.channel)?;
                }
                ResinType::Density | ResinType::Number => {
                    // One leaf pair per unique comparison found in clause bodies.
                    let comparisons = self.collect_comparisons_for(&source.name);
                    let mut registry_entry: Vec<(f64, bool, String)> = Vec::new();

                    for comp in comparisons {
                        let idx_normal = self.manager.create_leaf(
                            &comp.canonical_name,
                            Vector::zeros(1),
                            0.0,
                        );
                        let idx_inverted = self.manager.create_leaf(
                            &format!("-{}", comp.canonical_name),
                            Vector::ones(1),
                            0.0,
                        );
                        self.manager
                            .read_dual(idx_normal, idx_inverted, &comp.canonical_name)?;

                        registry_entry.push((
                            comp.threshold,
                            comp.is_upper_tail(),
                            comp.canonical_name.clone(),
                        ));
                    }

                    self.comparison_registry
                        .insert(source.name.clone(), registry_entry);
                }
            }
        }

        for clause in &self.clauses {
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

    /// Returns all unique comparison literals across all clauses that reference `source_name`.
    fn collect_comparisons_for(&self, source_name: &str) -> Vec<ComparisonLiteral> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for clause in &self.clauses {
            for comp in &clause.comparison_literals {
                if comp.source_atom == source_name
                    && seen.insert(comp.canonical_name.clone())
                {
                    result.push(comp.clone());
                }
            }
        }
        result
    }

    /// Returns the typed writer for a declared source, pre-configured with all
    /// comparisons found in the program's clauses.
    pub fn make_writer_for(
        &mut self,
        source_name: &str,
    ) -> Result<TypedWriter, Box<dyn std::error::Error>> {
        let source = self
            .sources
            .iter()
            .find(|s| s.name == source_name)
            .ok_or_else(|| format!("Source '{}' not found", source_name))?;

        match source.message_type {
            ResinType::Probability => Ok(TypedWriter::Probability(
                self.manager.make_probability_writer(&source.channel)?,
            )),
            ResinType::Boolean => Ok(TypedWriter::Boolean(
                self.manager.make_boolean_writer(&source.channel)?,
            )),
            ResinType::Density => {
                let channels = self
                    .comparison_registry
                    .get(source_name)
                    .cloned()
                    .unwrap_or_default();
                Ok(TypedWriter::Density(
                    self.manager.make_density_writer_for_channels(&channels),
                ))
            }
            ResinType::Number => {
                let channels = self
                    .comparison_registry
                    .get(source_name)
                    .cloned()
                    .unwrap_or_default();
                Ok(TypedWriter::Number(
                    self.manager.make_number_writer_for_channels(&channels),
                ))
            }
        }
    }

    pub fn circuit_from_dnf(&self, dnf: Dnf, target_token: &str) {
        // Get indexing from name to foliage
        let index_map = self.manager.get_index_map();

        // A DNF is an OR over AND, i.e., a sum over products without further hirarchy
        let mut sum_product = Vec::new();
        for clause in &dnf.clauses {
            let mut product = vec![];

            for literal in clause {
                // Derived atoms (e.g. intermediate rules like `permitted`)
                // appear in stable models but have no corresponding leaf.
                // Only choice atoms have leaves; skip everything else.
                if let Some(&idx) = index_map.get(literal) {
                    product.push(idx as u32);
                }
            }

            sum_product.push(product);
        }

        // Add the target to the ReactiveCircuit
        self.manager
            .reactive_circuit
            .lock()
            .unwrap()
            .add_sum_product(&sum_product, target_token);
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
            comparison_registry: HashMap::new(),
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

    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::path::Path;
    use std::time::Instant;
    use std::{collections::HashMap, fmt::Debug};

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

    // -------------------------------------------------------------------
    // Density / Number / Boolean source tests
    // -------------------------------------------------------------------

    #[test]
    fn test_density_source_compilation() {
        let model = r#"
            distance(hospital) <- source("/distance/hospital", Density).
            safe if distance(hospital) < 20.0.
            safe if distance(hospital) > 55.0.
            safe -> target("/safety").
        "#;

        let mut resin = Resin::compile(model, 1, false).expect("Compile failed");

        // Two comparison leaf pairs should have been created
        let names = resin.manager.get_names();
        assert!(names.iter().any(|n| n.contains("lt")), "lt leaf missing");
        assert!(names.iter().any(|n| n.contains("gt")), "gt leaf missing");

        // The comparison registry should have two entries for distance(hospital)
        let registry = resin.comparison_registry.get("distance(hospital)").unwrap();
        assert_eq!(registry.len(), 2);
        let has_lt = registry.iter().any(|(_, upper_tail, _)| !upper_tail);
        let has_gt = registry.iter().any(|(_, upper_tail, _)| *upper_tail);
        assert!(has_lt, "lower-tail entry missing");
        assert!(has_gt, "upper-tail entry missing");

        // make_writer_for should return a Density writer
        let writer = resin.make_writer_for("distance(hospital)").unwrap();
        assert!(matches!(writer, crate::channels::ipc::TypedWriter::Density(_)));
    }

    #[test]
    fn test_density_writer_updates_leaves() {
        use std::thread::sleep;
        use std::time::Duration;

        let model = r#"
            dist <- source("/dist", Density).
            safe if dist < 20.0.
            safe if dist > 55.0.
            safe -> target("/safety").
        "#;

        let mut resin = Resin::compile(model, 1, false).expect("Compile failed");
        let writer = resin.make_writer_for("dist").unwrap();

        let TypedWriter::Density(density_writer) = writer else {
            panic!("Expected Density writer");
        };

        // Write a Normal(25, 5) distribution
        let dist = crate::channels::ipc::VectorDistribution::Normal {
            mean: crate::circuit::Vector::from_elem(1, 25.0),
            std: crate::circuit::Vector::from_elem(1, 5.0),
        };
        density_writer.write(&dist, None);
        sleep(Duration::from_millis(30));

        let values = resin.manager.get_values();
        let names = resin.manager.get_names();

        // Find the lt leaf value
        let lt_idx = names.iter().position(|n| n.contains("lt")).unwrap();
        let gt_idx = names.iter().position(|n| n.contains("gt")).unwrap();

        // P(X < 20) for Normal(25, 5) ≈ 0.159
        assert!((values[lt_idx][0] - 0.159).abs() < 0.001,
            "lt leaf = {}", values[lt_idx][0]);
        // P(X > 55) for Normal(25, 5) ≈ 0 (extremely small)
        assert!(values[gt_idx][0] < 1e-6,
            "gt leaf = {}", values[gt_idx][0]);
    }

    #[test]
    fn test_number_source_compilation() {
        let model = r#"
            speed <- source("/speed", Number).
            moving if speed > 5.0.
            moving -> target("/moving").
        "#;

        let mut resin = Resin::compile(model, 1, false).expect("Compile failed");
        let writer = resin.make_writer_for("speed").unwrap();
        assert!(matches!(writer, crate::channels::ipc::TypedWriter::Number(_)));

        let TypedWriter::Number(num_writer) = writer else {
            panic!("Expected Number writer");
        };

        // value > 5.0 → 1.0; value < 5.0 → 0.0
        use std::thread::sleep;
        use std::time::Duration;

        num_writer.write(Vector::from(vec![10.0]), None);
        sleep(Duration::from_millis(30));
        let values = resin.manager.get_values();
        let names = resin.manager.get_names();
        let gt_idx = names.iter().position(|n| n.contains("gt")).unwrap();
        assert_eq!(values[gt_idx][0], 1.0, "speed=10 should be > 5");

        num_writer.write(Vector::from(vec![2.0]), None);
        sleep(Duration::from_millis(30));
        let values = resin.manager.get_values();
        assert_eq!(values[gt_idx][0], 0.0, "speed=2 should not be > 5");
    }

    #[test]
    fn test_boolean_source_compilation() {
        let model = r#"
            active <- source("/active", Boolean).
            alarm if active.
            alarm -> target("/alarm").
        "#;

        let mut resin = Resin::compile(model, 1, false).expect("Compile failed");
        let writer = resin.make_writer_for("active").unwrap();
        assert!(matches!(writer, crate::channels::ipc::TypedWriter::Boolean(_)));

        let TypedWriter::Boolean(bool_writer) = writer else {
            panic!("Expected Boolean writer");
        };

        use std::thread::sleep;
        use std::time::Duration;

        bool_writer.write(true, None);
        sleep(Duration::from_millis(30));
        let values = resin.manager.get_values();
        let names = resin.manager.get_names();
        let active_idx = names.iter().position(|n| n == "active").unwrap();
        assert_eq!(values[active_idx][0], 1.0);

        bool_writer.write(false, None);
        sleep(Duration::from_millis(30));
        let values = resin.manager.get_values();
        assert_eq!(values[active_idx][0], 0.0);
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
        let _ = resin
            .manager
            .reactive_circuit
            .lock()
            .unwrap()
            .to_combined_svg("output/test/test_resin_model_circuits.svg");

        println!(
            "{:#?}",
            resin.manager.reactive_circuit.lock().unwrap().targets
        );

        // Count the correct number of Resin elements
        assert_eq!(resin.clauses.len(), 3);
        assert_eq!(resin.sources.len(), 0);
        assert_eq!(resin.targets.len(), 1);

        // Check a correct result for target signal
        let result = resin.manager.reactive_circuit.lock().unwrap().update();
        assert_eq!(result["unsafe"], Vector::from(vec![0.94]));
    }
}
