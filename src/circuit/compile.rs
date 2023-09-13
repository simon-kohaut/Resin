use crate::circuit::leaf::Leaf;
use crate::circuit::ReactiveCircuit;
use crate::circuit::{shared_leaf, Model, SharedLeaf};
use crate::language::Resin;
use clap::Parser;
use clingo::{control, Control, ModelType, Part, ShowType, SolveMode};
use std::panic;

use super::SharedReactiveCircuit;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The Resin source to apply
    #[arg(short, long)]
    pub source: String,
}

fn solve(ctl: Control, rc: SharedReactiveCircuit, resin: &mut Resin) {
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

                let mut model = Model::new(&vec![], &None);
                println!("");

                for source in &resin.sources {
                    // Check if source atom is in symbols
                    // If it is, add it with an initial probability of 0
                    for symbol in &atoms {
                        let name = format!("{}", symbol);
                        if source.name == name {
                            let leaf: SharedLeaf;
                            if resin.leafs.get(&name).is_none() {
                                leaf = Leaf::new(&0.0, &0.0, &name).share();
                                resin.leafs.insert(name, leaf.clone());
                            } else {
                                leaf = resin.leafs.get(&name).unwrap().clone();
                            }
                            model.append(&leaf);
                        }
                    }

                    // Check if source atom is in complementary symbols
                    // If it is, add it with an initial probability of 1
                    for symbol in &complement {
                        let name = format!("¬{}", symbol);
                        if source.name == name {
                            let leaf: SharedLeaf;
                            if resin.leafs.get(&name).is_none() {
                                leaf = Leaf::new(&1.0, &0.0, &name).share();
                                resin.leafs.insert(name, leaf.clone());
                            } else {
                                leaf = resin.leafs.get(&name).unwrap().clone();
                            }
                            model.append(&leaf);
                        }
                    }
                }

                for clause in &resin.clauses {
                    // Clauses that are deterministic do not need to be included in model
                    if clause.probability.is_none() {
                        continue;
                    }

                    // Check if clause atom is in symbols
                    // If it is, check if conditions are met and add its conditional probability
                    for symbol in &atoms {
                        let name = format!("{}", symbol);
                        let node_name = format!("P({} | {})", name, clause.body.join(", "));
                        if clause.head == name {
                            // If the conditions in this clause are violated,
                            // We do not add the conditional probability
                            let mut conditions_met = true;
                            for condition in &clause.body {
                                for complementary in &complement {
                                    if condition == complementary.name().unwrap() {
                                        conditions_met = false;
                                        break;
                                    }
                                }
                            }

                            if conditions_met {
                                let leaf: SharedLeaf;
                                if resin.leafs.get(&node_name).is_none() {
                                    leaf =
                                        shared_leaf(clause.probability.unwrap(), 0.0, &node_name);
                                    resin.leafs.insert(node_name.clone(), leaf.clone());
                                } else {
                                    leaf = resin.leafs.get(&node_name).unwrap().clone();
                                }
                                model.append(&leaf);
                                println!("Added {} = {}", node_name, clause.probability.unwrap());
                            }
                        }
                    }

                    // Check if source atom is in complementary symbols
                    // If it is, add it with an initial probability of 1
                    for symbol in &complement {
                        let name = format!("¬{}", symbol);
                        let node_name = format!("P({} | {})", name, clause.body.join(", "));
                        if clause.head == name[2..] {
                            let leaf: SharedLeaf;
                            if resin.leafs.get(&node_name).is_none() {
                                leaf =
                                    shared_leaf(1.0 - clause.probability.unwrap(), 0.0, &node_name);
                                resin.leafs.insert(node_name.clone(), leaf.clone());
                            } else {
                                leaf = resin.leafs.get(&node_name).unwrap().clone();
                            }
                            model.append(&leaf);
                            println!("Added {} = {}", node_name, clause.probability.unwrap());
                        }
                    }
                }

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
        let rc = ReactiveCircuit::empty_new().share();

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
        solve(ctl, rc.clone(), &mut resin);

        // let not_rain = resin.leafs.get("¬rain").unwrap().clone();
        // let rain = resin.leafs.get("rain").unwrap().clone();
        // let not_speed = resin.leafs.get("¬speed").unwrap().clone();
        // let speed = resin.leafs.get("speed").unwrap().clone();
        // let not_clearance = leafs.get("¬clearance").unwrap().clone();
        // let clearance = resin.leafs.get("clearance").unwrap().clone();

        // let not_high_speed = leafs.get("¬high_speed").unwrap().clone();
        // let high_speed = leafs.get("high_speed").unwrap().clone();

        // let not_sunny = resin.leafs.get("¬sunny").unwrap().clone();
        // let sunny = resin.leafs.get("sunny").unwrap().clone();
        // let not_cloudy = resin.leafs.get("¬cloudy").unwrap().clone();
        // let cloudy = resin.leafs.get("cloudy").unwrap().clone();
        // let not_day = resin.leafs.get("¬day").unwrap().clone();
        // let day = resin.leafs.get("day").unwrap().clone();
        // let not_raining = resin.leafs.get("¬raining").unwrap().clone();
        // let raining = resin.leafs.get("raining").unwrap().clone();
        // let not_grass_long_1 = resin.leafs.get("¬grass_long(l1)").unwrap().clone();
        // let grass_long_1 = resin.leafs.get("grass_long(l1)").unwrap().clone();
        // let not_grass_long_2 = resin.leafs.get("¬grass_long(l2)").unwrap().clone();
        // let grass_long_2 = resin.leafs.get("grass_long(l2)").unwrap().clone();
        // let lawn_1 = resin.leafs.get("lawn(l1)").unwrap().clone();
        // let lawn_2 = resin.leafs.get("lawn(l2)").unwrap().clone();

        let _ = rc.lock().unwrap().to_svg("0");
        println!("{}", rc.lock().unwrap().get_value());

        // lift![rc, raining, not_raining];
        // let _ = rc.lock().unwrap().to_svg("1".to_string());
        // println!("{}", rc.lock().unwrap().get_value());

        // lift![rc, raining, not_raining];
        // lift![rc, cloudy, not_cloudy, sunny, not_sunny];
        // let _ = rc.lock().unwrap().to_svg("2".to_string());
        // println!("{}", rc.lock().unwrap().get_value());

        // drop![rc, lawn_1, lawn_2];
        // update(not_raining.clone(), 0.7);
        // update(raining.clone(), 0.3);
        // update(grass_long_1.clone(), 1.0);
        // update(not_grass_long_1.clone(), 0.0);

        // let _ = rc.lock().unwrap().to_svg("3".to_string());

        // println!("{}", rc.lock().unwrap().get_value());

        // rc = rc.drop(vec![lawn_1.clone(), lawn_2.clone(), day.clone(), not_day.clone()]);
        // rc = rc.lift(vec![sunny.clone(), cloudy.clone(), not_sunny.clone(), not_cloudy.clone(), raining.clone(), not_raining.clone()]);
        // lift![rc, not_speed, speed];
        // println!("{}", rc.lock().unwrap().get_value());

        // // drop![rc, rain, not_rain];
        // update(rain.clone(), 0.2);

        // println!("{}", rc.lock().unwrap().get_value());
        // let _ = rc.lock().unwrap().to_svg("fitted_resin".to_string());
    }

    // Create a Reactive Circuits for each target signal
    return Vec::new();
}
