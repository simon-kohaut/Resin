use std::str::FromStr;

use rclrs::RclrsError;

use super::{Clause, Source, Target};
use crate::channels::manager::Manager;
use crate::circuit::category::Category;
use crate::circuit::reactive::ReactiveCircuit;
use crate::language::{asp::solve, Dnf};

pub struct Resin {
    pub circuits: Vec<ReactiveCircuit>,
    pub clauses: Vec<Clause>,
    pub sources: Vec<Source>,
    pub targets: Vec<Target>,
    pub manager: Manager,
}

impl Resin {
    pub fn compile(model: &str) -> Result<Resin, RclrsError> {
        // Parse and setup Resin runtime environment
        let mut resin: Resin = model.parse().unwrap();
        resin.setup_signals()?;

        // Pass data to Clingo and obtain stable models
        for target_index in 0..resin.targets.len() {
            // Compile Resin into ASP
            let program = resin.to_asp(target_index);

            // Solve ASP and obtain DNF formula from which the target is removed
            let mut dnf = solve(&program);
            dnf.remove(&resin.targets[target_index].name);

            // Build the RC from the DNF
            let mut rc = resin.circuit_from_dnf(dnf);
            rc.set_dependencies(&mut resin.manager, None, vec![]);
            resin.circuits.push(rc);
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

    pub fn setup_signals(&mut self) -> Result<(), RclrsError> {
        // Create all source channels and parameter leafs
        for source in &self.sources {
            let category = Category::new(&source.name, 0.0);

            let index = self.manager.create_leaf(
                &category.leafs[0].name,
                category.leafs[0].get_value(),
                0.0,
            );
            self.manager.read(index as u16, &source.channel, false)?;

            let index = self.manager.create_leaf(
                &category.leafs[1].name,
                category.leafs[1].get_value(),
                0.0,
            );
            self.manager.read(index as u16 + 1, &source.channel, true)?;
        }

        for clause in &self.clauses {
            // Clauses that are deterministic do not need to be included in model
            if clause.probability.is_none() {
                continue;
            }

            let category = Category::new(&clause.head, clause.probability.unwrap());

            self.manager
                .create_leaf(&category.leafs[0].name, category.leafs[0].get_value(), 0.0);
            self.manager
                .create_leaf(&category.leafs[1].name, category.leafs[1].get_value(), 0.0);
        }

        Ok(())
    }

    pub fn circuit_from_dnf(&self, dnf: Dnf) -> ReactiveCircuit {
        // Get indexing from name to foliage
        let index_map = self.manager.get_index_map();

        // A DNF is an OR over AND, i.e., a sum over products without further hirarchy
        let mut rc = ReactiveCircuit::new();
        for clause in &dnf.clauses {
            let mut product = vec![];

            for literal in clause {
                let index = index_map
                    .get(literal)
                    .expect("DNF contained literal that is not in Resin!");
                product.push(*index as u16);
            }

            rc.products.push((product, None));
        }

        rc
    }
}

impl FromStr for Resin {
    type Err = RclrsError;

    fn from_str(input: &str) -> Result<Resin, Self::Err> {
        let mut resin = Resin {
            circuits: vec![],
            clauses: vec![],
            sources: vec![],
            targets: vec![],
            manager: Manager::new(),
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

        // Setup data distribution through signal leafs
        resin.setup_signals()?;

        Ok(resin)
    }
}

#[cfg(test)]
mod tests {

    use std::collections::HashMap;

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
        let resin = Resin::compile(model);
        assert!(resin.is_ok());
        let mut resin = resin.unwrap();

        // Count the correct number of Resin elements
        assert_eq!(resin.clauses.len(), 3);
        assert_eq!(resin.sources.len(), 0);
        assert_eq!(resin.targets.len(), 1);
        assert_eq!(resin.circuits.len(), 1);

        // Check a correct result for target signal
        resin.circuits[0].update(&resin.manager.get_values());
        assert_eq!(resin.circuits[0].value(), 0.94);
    }

    #[test]
    fn test_simulation() {
        use itertools::Itertools;
        use polars::io::mmap::MmapBytesReader;
        use polars::prelude::*;
        use std::time::Instant;

        // Load CSV file from simulation
        print!("Load data in ... ");
        let clock = Instant::now();
        let file = std::fs::File::open("pairwise_distances.csv").unwrap();
        let file = Box::new(file) as Box<dyn MmapBytesReader>;
        let reader = CsvReader::new(file);
        let data = reader.with_delimiter(b',').finish().unwrap();
        println!("{}s", clock.elapsed().as_secs_f64());

        // Get unique drone names
        let mut drones = data.column("d1").unwrap().unique().unwrap();
        drones
            .extend(&data.column("d2").unwrap())
            .expect("Loading drone data failed!");
        drones = drones.unique().unwrap();

        let drone_names: Vec<&str> = drones
            .iter()
            .map(|drone| {
                let AnyValue::Utf8(name) = drone else { panic!("") };
                name
            })
            .collect();

        print!("Build Resin model in ... ");
        let clock = Instant::now();
        let mut model = "unsafe if close(X,Y).\nunsafe -> target(\"/safety\").\n".to_string();
        for drone_pair in drone_names.iter().combinations(2) {
            let d1 = drone_pair[0];
            let d2 = drone_pair[1];

            model += &format!("close({d1},{d2}) <- source(\"/ads_b/{d1}_{d2}\", Probability).\n");
        }
        println!("{}s", clock.elapsed().as_secs_f64());
        println!("{model}");

        print!("Compile Resin in ... ");
        let clock = Instant::now();
        let mut resin = Resin::compile(&model).expect("Could not compile Resin!");
        println!("{}s", clock.elapsed().as_secs_f64());

        print!("Update value ... ");
        let clock = Instant::now();
        resin.circuits[0].update(&resin.manager.get_values());
        println!("{}s", clock.elapsed().as_secs_f64());

        println!(
            "#operations {}",
            resin.circuits[0].counted_update(&resin.manager.get_values())
        );
        println!("#models {}", resin.circuits[0].products.len());
        println!("Size {}B", resin.circuits[0].size());
        println!("Value {}", resin.circuits[0].value());

        print!("Setup writers in ... ");
        let mut value_channels = HashMap::new();
        let clock = Instant::now();
        for drone_pair in drone_names.iter().combinations(2) {
            let d1 = drone_pair[0].to_owned();
            let d2 = drone_pair[1].to_owned();

            let channel = format!("/ads_b/{d1}_{d2}");
            let frequency = 10.0;
            let value = resin.manager.write(&channel, frequency).expect("Could not setup writer to data channel!");
            value_channels.insert((d1, d2), value);
        }
        println!("{}s", clock.elapsed().as_secs_f64());

        // Get individual timestamps for which data was stored
        let timestamp_series = data.column("t").unwrap();
        let unique_timestamps = timestamp_series.unique().unwrap();
        let timestamps = unique_timestamps.f64().unwrap();

        print!("Run simulation ... ");
        let mut runtimes = vec![];
        for timestep in timestamps {
            // Update simulation time
            let simulation_time;
            match timestep {
                Some(t) => simulation_time = t,
                None => break
            }

            // Distribute new data
            resin.manager.spin_once();

            let mask = timestamp_series.equal(simulation_time).unwrap();
            let mut current = data.filter(&mask).unwrap();
            current.as_single_chunk_par();

            let d1_array = current.column("d1").unwrap().utf8().unwrap();
            let d2_array = current.column("d2").unwrap().utf8().unwrap();
            let p_close_array = current.column("p_close").unwrap().f64().unwrap();
            for i in 0..current.height() {
                let d1 =  d1_array.get(i).unwrap();
                let d2 =  d2_array.get(i).unwrap();

                let value = match value_channels.get(&(d1, d2)) {
                    Some(v) => v,
                    None => value_channels.get(&(d2, d1)).unwrap()
                };
                *value.lock().unwrap() = p_close_array.get(i).unwrap();                    
            }

            let clock = Instant::now();
            resin.circuits[0].update(&resin.manager.get_values());
            runtimes.push(clock.elapsed().as_secs_f64());

            println!("Time {simulation_time} | Runtime {}", runtimes[runtimes.len() - 1]);
        }

    }
}
