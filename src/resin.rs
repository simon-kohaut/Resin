use crate::language::{Resin, Clause, ResinType, Source, Target};
use crate::circuit::{SharedLeaf, shared_leaf, Model};
use crate::circuit::ReactiveCircuit;
use clap::Parser;
use clingo::{control, Control, ModelType, Part, ShowType, SolveMode};
use std::panic;
use std::collections::HashMap;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The Resin source to apply
    #[arg(short, long)]
    pub source: String,
}

fn solve(ctl: Control, leafs: &HashMap<String, SharedLeaf>, rc: &mut ReactiveCircuit) {
    // get a solve handle
    let mut handle = ctl
        .solve(SolveMode::YIELD, &[])
        .expect("Failed retrieving solve handle.");

    // loop over all models
    loop {
        handle.resume().expect("Failed resume on solve handle.");
        match handle.model() {
            Ok(Some(model)) => {
                // get model type
                let model_type = model.model_type().unwrap();

                let type_string = match model_type {
                    ModelType::StableModel => "Stable model",
                    ModelType::BraveConsequences => "Brave consequences",
                    ModelType::CautiousConsequences => "Cautious consequences",
                };

                // get running number of model
                let number = model.number().unwrap();

                print!("Stable model {}:", number);
                let atoms = model
                    .symbols(ShowType::ATOMS)
                    .expect("Failed to retrieve symbols in the model.");

                let mut model = Model::new(vec![], None);
                for symbol in atoms {
                    print!(" {}", symbol);
                    model.append(leafs.get(symbol.name().unwrap()).unwrap().clone());
                }
                for leaf in leafs.values() {
                    if !model.contains(leaf.clone()) {
                        match leafs.get(&format!("¬{}", leaf.lock().unwrap().name)) {
                            Some(negated_leaf) => model.append(negated_leaf.clone()),
                            None => ()
                        }
                    }
                }
                rc.add_model(model);

                println!();
            }
            Ok(None) => {
                // stop if there are no more models
                break;
            }
            Err(e) => {
                panic!("Error: {}", e);
            }
        }
    }

    // close the solve handle
    handle.close().expect("Failed to close solve handle.");
}

pub fn parse(model: String) -> Vec<ReactiveCircuit> {
    let resin = model.parse::<Resin>().unwrap();

    // Pass data to Clingo and obtain stable models
    for target_index in 0..resin.targets.len() {
        let mut rc = ReactiveCircuit::new();
        let mut leafs = HashMap::new();
        for symbol in &resin.symbols {
            let negated_symbol = format!("¬{}", symbol);
            leafs.insert(symbol.to_string(), shared_leaf(1.0, 0.0, symbol.to_string()));
            leafs.insert(negated_symbol.to_string(), shared_leaf(1.0, 0.0, negated_symbol.to_string()));
        }

        let program = resin.to_asp(target_index);
        println!("\n{}\n", &program);

        let mut ctl =
            control(vec!["--models=0".to_string()]).expect("Failed creating clingo_control.");
        ctl.add("base", &[], &program)
            .expect("Failed to add a logic program.");

        // ground the base part
        let part = Part::new("base", vec![]).unwrap();
        let parts = vec![part];
        ctl.ground(&parts)
            .expect("Failed to ground a logic program.");

        // solve
        solve(ctl, &leafs, &mut rc);

        let not_day = leafs.get("¬day").unwrap().clone();
        let day = leafs.get("day").unwrap().clone();
        let not_raining = leafs.get("¬raining").unwrap().clone();
        let raining = leafs.get("raining").unwrap().clone();
        let not_grass_long: std::sync::Arc<std::sync::Mutex<crate::circuit::Leaf>> = leafs.get("¬grass_long").unwrap().clone();
        let grass_long: std::sync::Arc<std::sync::Mutex<crate::circuit::Leaf>> = leafs.get("grass_long").unwrap().clone();
        rc = rc.drop(vec![day, not_day, grass_long, not_grass_long]);
        rc = rc.lift(vec![raining, not_raining]);

        rc.to_svg("resin".to_string());
    }

    // Create a Reactive Circuits for each target signal
    return Vec::new();
}
