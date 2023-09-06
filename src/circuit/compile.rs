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

fn solve(ctl: Control, rc: &mut ReactiveCircuit, target: &String) -> HashMap<String, SharedLeaf>{
    // get a solve handle
    let mut handle = ctl
        .solve(SolveMode::YIELD, &[])
        .expect("Failed retrieving solve handle.");

    let mut leafs: HashMap<String, SharedLeaf> = HashMap::new();
    
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

                print!("{} {}:", type_string, number);
                let atoms = model
                    .symbols(ShowType::ATOMS)
                    .expect("Failed to retrieve positive symbols in the model.");

                let complement = model
                    .symbols(ShowType::COMPLEMENT | ShowType::ALL)
                    .expect("Failed to retrieve complementary symbols in the model.");

                let mut model = Model::new(vec![], None);
                println!("");     
                for symbol in complement {
                    let name = format!("¬{}", symbol);
                    if name == target.to_string() {
                        continue;
                    }
                    
                    print!(" {}", name);
                    let mut leaf: SharedLeaf;
                    if leafs.get(&name).is_none() {
                        leaf = shared_leaf(1.0, 0.0, name.clone());
                        leafs.insert(name, leaf.clone());
                    } else {
                        leaf = leafs.get(&name).unwrap().clone();
                    }
                    model.append(leaf)
                }
                println!("");
                
                for symbol in atoms {
                    let name = format!("{}", symbol);
                    if name == target.to_string() {
                        continue;
                    }
                    
                    print!(" {}", name);
                    let mut leaf: SharedLeaf;
                    if leafs.get(&name).is_none() {
                        leaf = shared_leaf(1.0, 0.0, name.clone());
                        leafs.insert(name, leaf.clone());
                        print!{"-inserted"}
                    } else {
                        leaf = leafs.get(&name).unwrap().clone();
                    }
                    model.append(leaf) 
                }
                println!("");     

                rc.add_model(model);

                println!();
            }
            Ok(None) => {
                // stop if there are no more models
                return leafs;
            }
            Err(e) => {
                panic!("Error: {}", e);
            }
        }
    }

    // close the solve handle
    handle.close().expect("Failed to close solve handle.");
}

pub fn compile(model: String) -> Vec<ReactiveCircuit> {
    let resin = model.parse::<Resin>().unwrap();

    // Pass data to Clingo and obtain stable models
    for target_index in 0..resin.targets.len() {
        let mut rc = ReactiveCircuit::new();

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
        let leafs = solve(ctl, &mut rc, &resin.targets[target_index].name);

        // let not_high_speed = leafs.get("¬high_speed").unwrap().clone();
        // let high_speed = leafs.get("high_speed").unwrap().clone();
        let not_sunny = leafs.get("¬sunny").unwrap().clone();
        let sunny = leafs.get("sunny").unwrap().clone();
        let not_cloudy = leafs.get("¬cloudy").unwrap().clone();
        let cloudy = leafs.get("cloudy").unwrap().clone();
        let not_day = leafs.get("¬day").unwrap().clone();
        let day = leafs.get("day").unwrap().clone();
        let not_raining = leafs.get("¬raining").unwrap().clone();
        let raining = leafs.get("raining").unwrap().clone();
        let not_grass_long_1 = leafs.get("¬grass_long(l1)").unwrap().clone();
        let grass_long_1 = leafs.get("grass_long(l1)").unwrap().clone();
        let not_grass_long_2 = leafs.get("¬grass_long(l2)").unwrap().clone();
        let grass_long_2 = leafs.get("grass_long(l2)").unwrap().clone();
        let lawn_1 = leafs.get("lawn(l1)").unwrap().clone();
        let lawn_2 = leafs.get("lawn(l2)").unwrap().clone();
       
        let _ = rc.to_svg("wmc_resin".to_string());

        // rc = rc.drop(vec![lawn_1.clone(), lawn_2.clone(), day.clone(), not_day.clone()]);
        rc = rc.lift(vec![sunny.clone(), cloudy.clone(), not_sunny.clone(), not_cloudy.clone(), raining.clone(), not_raining.clone()]);
        rc = rc.lift(vec![raining.clone(), not_raining.clone()]);

        let _ = rc.to_svg("fitted_resin".to_string());
    }

    // Create a Reactive Circuits for each target signal
    return Vec::new();
}
