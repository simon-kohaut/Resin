use std::panic;

use rclrs::RclrsError;
use clap::Parser;
use clingo::{control, Control, Part, ShowType, SolveMode};

use crate::circuit::reactive::ReactiveCircuit;
use crate::language::Resin;


#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The Resin source to apply
    #[arg(short, long)]
    pub source: String,
}


fn solve(ctl: Control, rc: &mut ReactiveCircuit, resin: &mut Resin) -> Result<(), RclrsError> {
    // get a solve handle
    let mut handle = ctl
        .solve(SolveMode::YIELD, &[])
        .expect("Failed retrieving solve handle.");

    // Map leaf names to their foliage index
    let index_map = resin.manager.get_index_map();

    // TODO
    // - Build RC from different forms
    // from_dnf method
    // from_cnf method
    // from_ddnnf method
    //
    // - Encapsulate Clingo part better
    // solve returns DNF
    // 
    // - Convert forms using own code and DSharp
    // dnf_to_cnf method
    // dnf_to_ddnnf method
    // cnf_to_ddnnf method
    //
    // - Proper CLI interface
    // - Proper documentation
    // - Proper Dockerfile/Readme instructions

    // loop over all models
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

                // println!(
                //     "Positive: {:?}",
                //     atoms
                //         .iter()
                //         .map(|symbol| format!("{}", symbol))
                //         .collect::<Vec<String>>()
                // );
                // println!(
                //     "Negative: {:?}",
                //     complement
                //         .iter()
                //         .map(|symbol| format!("{}", symbol))
                //         .collect::<Vec<String>>()
                // );

                // The product/model to add in this iteration
                let mut product = vec![];

                // Positive atoms
                for symbol in &atoms {
                    let name = format!("{}", symbol);
                    match index_map.get(&name) {
                        Some(index) => product.push(*index as u16),
                        None => (),
                    }
                }

                // Negated atoms
                for symbol in &complement {
                    let name = format!("¬{}", symbol);
                    match index_map.get(&name) {
                        Some(index) => product.push(*index as u16),
                        None => (),
                    }
                }

                rc.products.push((product, None));
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

    Ok(())
}

pub fn compile(model: String) -> Result<Resin, RclrsError> {
    // Parse and setup Resin runtime environment
    let mut resin: Resin = model.parse().unwrap();
    resin.setup_signals()?;

    // Pass data to Clingo and obtain stable models
    for target_index in 0..resin.targets.len() {
        // Compile Resin into ASP
        let program = resin.to_asp(target_index);

        // Setup Clingo solver
        let mut ctl =
            control(vec!["--models=0".to_string()]).expect("Failed creating clingo_control.");
        ctl.add("base", &[], &program)
            .expect("Failed to add a logic program.");

        // Ground the program
        let part = Part::new("base", vec![]).unwrap();
        ctl.ground(&vec![part])
            .expect("Failed to ground the logic program.");

        // Solve and build RC
        let mut rc = ReactiveCircuit::new();
        solve(ctl, &mut rc, &mut resin)?;
        rc.set_dependencies(&mut resin.manager, None, vec![]);
        resin.circuits.push(rc);
    }

    // Return the compiled Resin program
    Ok(resin)
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;

    use super::compile;

    #[test]
    fn test_model() {
        let model = "
        close(a,b) <- P(0.8).
        close(a,c) <- P(0.7).
        
        unsafe if close(X,Y).
        
        unsafe -> target(\"/safety\").
        ";

        // Compile Resin runtime environment
        let resin = compile(model.to_owned());
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

        // // Get individual timestamps for which data was stored
        // let timestamps = data.column("t").unwrap().unique().unwrap();
        // let AnyValue::Float64(start) = timestamps.get(0) else { panic!("") };

        // // Setup data distribution
        // let manager = Manager::new();

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
        let mut resin = compile(model.to_owned()).expect("Could not compile Resin!");
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
    }
}
