use std::rc;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rclrs::RclrsError;

use super::{Clause, Source, Target};
use crate::channels::manager::Manager;
use crate::circuit::category::Category;
use crate::circuit::reactive::{ReactiveCircuit, SharedCircuit, DeployedCircuit};
use crate::language::{asp::solve, Dnf};

pub type SharedStorage = Arc<Mutex<Vec<f64>>>;

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

        // Pass data to Clingo and obtain stable models
        for target_index in 0..resin.targets.len() {
            // Compile Resin into ASP
            let program = resin.to_asp(target_index);

            // Solve ASP and obtain DNF formula from which the target is removed
            let mut dnf = solve(&program);
            dnf.remove(&resin.targets[target_index].name);

            // Build the RC from the DNF
            resin.circuits.push(resin.circuit_from_dnf(dnf));
        }

        // Return the compiled Resin program
        Ok(resin)
    }

    fn deploy_helper(&self, rc: &SharedCircuit, indices: Option<Vec<usize>>) -> Vec<DeployedCircuit> {
        // Extend indices
        let mut indices = indices.unwrap_or_default();
        indices.push(rc.lock().unwrap().index);

        // For each RC in this target graph, deploy
        let rc_guard = rc.lock().unwrap();

        // If this is a const 1, do not deploy
        if rc_guard.products.is_empty() {
            return vec![];
        }

        let mut deployed = vec![rc_guard.deploy()];
        for (factors, sub_rc) in &rc_guard.products {
            let mut foliage = self.manager.foliage.lock().unwrap();
            for leaf in factors {
                foliage[*leaf as usize].add_dependencies(&indices);
            }
            drop(foliage);

            deployed.append(&mut self.deploy_helper(sub_rc, Some(indices.clone())));
        }

        deployed
    }

    pub fn deploy(
        &mut self,
        target: usize,
    ) -> (Vec<DeployedCircuit>, Vec<f64>) {
        // Get root and setup index
        let mut rc = self.circuits[target].clone();
        rc.recompute_index(0, 0);

        // Clear old index of leafs
        self.manager.clear_dependencies();

        // For each RC in this target graph, deploy
        let deployed = self.deploy_helper(&rc.share(), None);
        let mut storage = vec![0.0; deployed.len()];

        // Ensure that storage is ready for partial updates
        self.full_update(&deployed, &mut storage);

        (deployed, storage)
    }

    pub fn full_update(&self, deployed: &[DeployedCircuit], storage: &mut Vec<f64>) -> f64 {
        let leafs = self.manager.get_values();

        let clock = Instant::now();
        for index in (0..deployed.len()).rev() {
            storage[index] = deployed[index].update(&leafs, storage);
        }
        clock.elapsed().as_secs_f64()
    }

    pub fn update(&self, deployed: &[DeployedCircuit], storage: &mut Vec<f64>) -> (usize, f64) {
        let mut rc_queue = self.manager.rc_queue.lock().unwrap();
        let leafs = self.manager.get_values();
        let number_updates = rc_queue.len();

        let clock = Instant::now();
        for index in rc_queue.iter().rev() {
            storage[*index] = deployed[*index].update(&leafs, storage);
        }
        rc_queue.clear();
        (number_updates, clock.elapsed().as_secs_f64())
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
            let index = self.manager.create_leaf(
                &source.name,
                0.0,
                0.0,
            );
            self.manager.read(index as u16, &source.channel, false)?;

            let index = self.manager.create_leaf(
                &format!("-{}", source.name),
                1.0,
                0.0,
            );
            self.manager.read(index as u16, &source.channel, true)?;
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

            rc.products.insert(product, ReactiveCircuit::one().share());
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
        use std::fs::{File, OpenOptions};
        use std::io::Write;
        use std::path::Path;
        use std::time::Instant;

        use itertools::Itertools;
        use polars::io::mmap::MmapBytesReader;
        use polars::prelude::*;

        use crate::channels::clustering::{create_boundaries, frequency_adaptation};

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
        println!("Leafs {:?}", resin.manager.get_names());

        print!("Setup writers in ... ");
        let mut writers = HashMap::new();
        let clock = Instant::now();
        for drone_pair in drone_names.iter().combinations(2) {
            let d1 = drone_pair[0].to_owned();
            let d2 = drone_pair[1].to_owned();

            let channel = format!("/ads_b/{d1}_{d2}");
            let writer = resin
                .manager
                .make_writer(&channel)
                .expect("Could not setup writer to data channel!");
            writers.insert((d1, d2), writer);
        }
        println!("{}s", clock.elapsed().as_secs_f64());

        // Get individual timestamps for which data was stored
        let timestamp_series = data.column("t").unwrap();
        let unique_timestamps = timestamp_series.unique().unwrap();
        let timestamps = unique_timestamps.f64().unwrap();

        // Deploy RC
        let original = resin.circuits[0].deep_clone();
        let (deployed_original, mut original_storage) = resin.deploy(0);
        let (mut deployed_adapted, mut adapted_storage) = resin.deploy(0);

        print!("Run simulation ... ");
        let boundaries = create_boundaries(1.0, 1);
        let mut partitions = partitioning(&resin.manager.get_frequencies(), &boundaries);
        let mut inference_timestamps = vec![];
        let mut original_inference_times = vec![];
        let mut adapted_inference_times = vec![];
        let mut adapted_full_inference_times = vec![];
        let mut rc_numbers = vec![];
        let mut root_leafs = vec![];
        let mut number_root_leafs = original.leafs().len();
        let mut frequencies = vec![];
        for timestep in timestamps {
            // Update simulation time
            let simulation_time;
            match timestep {
                Some(t) => simulation_time = t,
                None => break,
            }

            // if simulation_time > 60.0 {
            //     break;
            // }

            // Get data for this timestep
            let mask = timestamp_series.equal(simulation_time).unwrap();
            let mut current = data.filter(&mask).unwrap();
            current.as_single_chunk_par();

            // Distribute new data
            let d1_array = current.column("d1").unwrap().utf8().unwrap();
            let d2_array = current.column("d2").unwrap().utf8().unwrap();
            let p_close_array = current.column("p_close").unwrap().f64().unwrap();
            for i in 0..current.height() {
                let d1 = d1_array.get(i).unwrap();
                let d2 = d2_array.get(i).unwrap();

                match writers.get(&(d1, d2)) {
                    Some(writer) => writer.write(p_close_array.get(i).unwrap(), Some(simulation_time)),
                    None => writers.get(&(d2, d1)).unwrap().write(p_close_array.get(i).unwrap(), Some(simulation_time)),
                };
            }

            // Make publish/subscribe cycle happen
            resin.manager.spin_once();
            resin.manager.prune_frequencies(1.0, Some(simulation_time));
            
            // Adapt RC if partitioning changes
            let new_partitions = partitioning(&resin.manager.get_frequencies(), &boundaries);
            if partitions != new_partitions {
                partitions = new_partitions;

                resin.circuits[0] = original.deep_clone();

                print!("Adapt leafs in ... ");
                let clock = Instant::now();
                let number_of_adaptations = frequency_adaptation(
                    &mut resin.circuits[0],
                    &partitions
                );
                println!(
                    "{}s for {} leafs.",
                    clock.elapsed().as_secs_f64(),
                    number_of_adaptations
                );

                if number_of_adaptations > 0 {
                    let depth = resin.circuits[0].depth(None);
                    let ops = resin.circuits[0].counted_update(&resin.manager.get_values());
                    let leafs = resin.circuits[0].leafs();
                    let leaf_names = resin.manager.get_names();
                    let high_frequency_leafs: Vec<String> = leafs
                        .iter()
                        .map(|l| leaf_names[*l as usize].clone())
                        .collect();
                    number_root_leafs = high_frequency_leafs.len();
                    println!("New depth {depth} and number of operations {ops} over leafs {high_frequency_leafs:?}");
    
                    // Deploy newly adapted RC
                    (deployed_adapted, adapted_storage) = resin.deploy(0);
                }
            }

            // Update value and note runtime for adapted
            let (rc_number, elapsed) = resin.update(&deployed_adapted, &mut adapted_storage);
            adapted_inference_times.push(elapsed);

            let elapsed = resin.full_update(&deployed_original, &mut original_storage);
            original_inference_times.push(elapsed);

            let elapsed = resin.full_update(&deployed_adapted, &mut adapted_storage);
            adapted_full_inference_times.push(elapsed);

            // Time update to value
            inference_timestamps.push(simulation_time);
            rc_numbers.push(rc_number);
            root_leafs.push(number_root_leafs);
            frequencies.push(resin.manager.get_frequencies());
            println!(
                "Time {simulation_time} | Original Runtime {} | Adapted Runtime {} | Adapted Full Runtime {}\n",
                original_inference_times[inference_timestamps.len() - 1],
                adapted_inference_times[inference_timestamps.len() - 1],
                adapted_full_inference_times[inference_timestamps.len() - 1]
            );
        }

        println!("Export results.");
        let path = Path::new("output/data/simulation_results.csv");
        if !path.exists() {
            let mut file = File::create(path).expect("Unable to create file");
            file.write_all("Time,OriginalRuntime,AdaptedRuntime,AdaptedFullRuntime,RCs,Leafs\n".as_bytes())
                .expect("Unable to write data");
        }

        let mut file = OpenOptions::new().append(true).open(path).unwrap();
        let mut csv_text = "".to_string();
        for i in 0..inference_timestamps.len() {
            csv_text.push_str(&format!(
                "{},{},{},{},{},{}\n",
                inference_timestamps[i], original_inference_times[i], adapted_inference_times[i], adapted_full_inference_times[i], rc_numbers[i], root_leafs[i]
            ));
        }
        file.write_all(csv_text.as_bytes())
            .expect("Unable to write data");

        let path = Path::new("output/data/simulation_frequencies_results.csv");
        if !path.exists() {
            let _ = File::create(path).expect("Unable to create file");
        }

        let mut file = OpenOptions::new().append(true).open(path).unwrap();
        let mut csv_text = "".to_string();
        // Frequencies header
        let num_leafs = resin.manager.get_frequencies().len();
        csv_text.push_str("Time");
        for i in 0..num_leafs {
                csv_text.push_str(&format!(",f{i}"));
        }
        csv_text.push_str("\n");
        // Frequencies
        for i in 0..inference_timestamps.len() {
            csv_text.push_str(&format!("{}", inference_timestamps[i]));
            for j in 0..num_leafs {
                csv_text.push_str(&format!(",{}", frequencies[i][j]));
            }
            csv_text.push_str("\n");
        }
        file.write_all(csv_text.as_bytes())
            .expect("Unable to write data");
    }
}
