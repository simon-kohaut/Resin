use crate::circuit::reactive_circuit::update;
use crate::circuit::ReactiveCircuit;
use crate::circuit::{shared_leaf, Model, SharedLeaf};
use crate::language::Resin;
use clap::Parser;
use clingo::{control, Control, ModelType, Part, ShowType, SolveMode};
use std::panic;
use std::sync::{Arc, Mutex};

use crate::{drop, lift};

use super::SharedReactiveCircuit;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The Resin source to apply
    #[arg(short, long)]
    pub source: String,
}

fn solve(ctl: Control, rc: SharedReactiveCircuit, resin: &mut Resin, target: &String) {
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
                    let leaf: SharedLeaf;
                    if resin.leafs.get(&name).is_none() {
                        leaf = shared_leaf(0.0, 0.0, name.clone());
                        resin.leafs.insert(name, leaf.clone());
                    } else {
                        leaf = resin.leafs.get(&name).unwrap().clone();
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
                    let leaf: SharedLeaf;
                    if resin.leafs.get(&name).is_none() {
                        leaf = shared_leaf(1.0, 0.0, name.clone());
                        resin.leafs.insert(name, leaf.clone());
                        print! {"-inserted"}
                    } else {
                        leaf = resin.leafs.get(&name).unwrap().clone();
                    }
                    model.append(leaf)
                }
                println!("");

                rc.lock().unwrap().models.push(model);
                for leaf in resin.leafs.values() {
                    leaf.lock().unwrap().circuits.push(rc.clone());
                }

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

pub fn compile(model: String) -> Vec<ReactiveCircuit> {
    let mut resin = model.parse::<Resin>().unwrap();

    // Pass data to Clingo and obtain stable models
    for target_index in 0..resin.targets.len() {
        let mut rc = ReactiveCircuit::empty_new().share();

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
        let target_name = resin.targets[target_index].name.clone();
        solve(ctl, rc.clone(), &mut resin, &target_name);

        for clause in &resin.clauses {
            match resin.leafs.get(&clause.head) {
                Some(leaf) => leaf.lock().unwrap().set_value(clause.probability),
                None => (),
            }

            match resin.leafs.get(&format!("¬{}", clause.head)) {
                Some(leaf) => leaf.lock().unwrap().set_value(1.0 - clause.probability),
                None => (),
            }
        }

        let not_rain = resin.leafs.get("¬rain").unwrap().clone();
        let rain = resin.leafs.get("rain").unwrap().clone();
        let not_speed = resin.leafs.get("¬speed").unwrap().clone();
        let speed = resin.leafs.get("speed").unwrap().clone();
        // let not_clearance = leafs.get("¬clearance").unwrap().clone();
        let clearance = resin.leafs.get("clearance").unwrap().clone();

        // let not_high_speed = leafs.get("¬high_speed").unwrap().clone();
        // let high_speed = leafs.get("high_speed").unwrap().clone();
        // let not_sunny = leafs.get("¬sunny").unwrap().clone();
        // let sunny = leafs.get("sunny").unwrap().clone();
        // let not_cloudy = leafs.get("¬cloudy").unwrap().clone();
        // let cloudy = leafs.get("cloudy").unwrap().clone();
        // let not_day = leafs.get("¬day").unwrap().clone();
        // let day = leafs.get("day").unwrap().clone();
        // let not_raining = leafs.get("¬raining").unwrap().clone();
        // let raining = leafs.get("raining").unwrap().clone();
        // let not_grass_long_1 = leafs.get("¬grass_long(l1)").unwrap().clone();
        // let grass_long_1 = leafs.get("grass_long(l1)").unwrap().clone();
        // let not_grass_long_2 = leafs.get("¬grass_long(l2)").unwrap().clone();
        // let grass_long_2 = leafs.get("grass_long(l2)").unwrap().clone();
        // let lawn_1 = leafs.get("lawn(l1)").unwrap().clone();
        // let lawn_2 = leafs.get("lawn(l2)").unwrap().clone();

        // update(rain.clone(), 0.3);

        let _ = rc.lock().unwrap().to_svg("wmc_resin".to_string());
        println!("{}", rc.lock().unwrap().get_value());

        // rc = rc.drop(vec![lawn_1.clone(), lawn_2.clone(), day.clone(), not_day.clone()]);
        // rc = rc.lift(vec![sunny.clone(), cloudy.clone(), not_sunny.clone(), not_cloudy.clone(), raining.clone(), not_raining.clone()]);
        lift![rc, not_speed, speed];
        println!("{}", rc.lock().unwrap().get_value());

        // drop![rc, rain, not_rain];
        update(rain.clone(), 0.2);

        println!("{}", rc.lock().unwrap().get_value());
        let _ = rc.lock().unwrap().to_svg("fitted_resin".to_string());
    }

    // Create a Reactive Circuits for each target signal
    return Vec::new();
}
