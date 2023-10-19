use crate::circuit::leaf::Leaf;
use crate::circuit::ReactiveCircuit;
use crate::circuit::{shared_leaf, Model, SharedLeaf};
use crate::language::Resin;
use clap::Parser;
use clingo::{control, Control, ModelType, Part, ShowType, SolveMode};
use std::panic;

use super::category::Category;
use super::leaf::activate_channel;
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
                // Get model type
                let model_type = model.model_type().unwrap();

                let type_string = match model_type {
                    ModelType::StableModel => "Stable model",
                    ModelType::BraveConsequences => "Brave consequences",
                    ModelType::CautiousConsequences => "Cautious consequences",
                };

                // Get running number of model
                let number = model.number().unwrap();

                print!("{} {}:", type_string, number);
                let atoms = model
                    .symbols(ShowType::ATOMS)
                    .expect("Failed to retrieve positive symbols in the model.");

                let complement = model
                    .symbols(ShowType::COMPLEMENT | ShowType::ALL)
                    .expect("Failed to retrieve complementary symbols in the model.");

                let mut model = Model::empty_new(&Some(rc.clone()));
                println!("");
                println!(
                    "Positive: {:?}",
                    atoms
                        .iter()
                        .map(|symbol| symbol.name().unwrap())
                        .collect::<Vec<&str>>()
                );
                println!(
                    "Negative: {:?}",
                    complement
                        .iter()
                        .map(|symbol| symbol.name().unwrap())
                        .collect::<Vec<&str>>()
                );

                for source in &resin.sources {
                    // Check if source atom is in symbols
                    // If it is, add it with an initial probability of 0
                    for symbol in &atoms {
                        let name = format!("{}", symbol);

                        if source.name == name {
                            match resin.leafs.get(&name) {
                                Some(leaf) => {
                                    model.append(leaf);
                                    println!("Added source {}", &leaf.lock().unwrap().name);
                                }
                                None => {
                                    let category = Category::new(&name);
                                    activate_channel(&category.leafs[0], &source.channel, &false);
                                    activate_channel(&category.leafs[1], &source.channel, &true);
                                    resin.leafs.insert(name, category.leafs[0].clone());
                                    resin.leafs.insert(
                                        category.leafs[1].lock().unwrap().name.to_owned(),
                                        category.leafs[1].clone(),
                                    );
                                    model.append(&category.leafs[0]);
                                    println!(
                                        "Added source {}",
                                        &category.leafs[0].lock().unwrap().name
                                    );
                                }
                            }
                        }
                    }

                    // Check if source atom is in complementary symbols
                    // If it is, add it with an initial probability of 1
                    for symbol in &complement {
                        let name = format!("{}", symbol);
                        if source.name == name {
                            match resin.leafs.get(&format!("¬{}", name)) {
                                Some(leaf) => {
                                    model.append(leaf);
                                    println!("Added source {}", &leaf.lock().unwrap().name);
                                }
                                None => {
                                    let category = Category::new(&name);
                                    activate_channel(&category.leafs[0], &source.channel, &false);
                                    activate_channel(&category.leafs[1], &source.channel, &true);
                                    resin.leafs.insert(name, category.leafs[0].clone());
                                    resin.leafs.insert(
                                        category.leafs[1].lock().unwrap().name.to_owned(),
                                        category.leafs[1].clone(),
                                    );
                                    model.append(&category.leafs[1]);
                                    println!(
                                        "Added source {}",
                                        &category.leafs[1].lock().unwrap().name
                                    );
                                }
                            }
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

                rc.lock().unwrap().add_model(&model);
                println!();
            }
            Ok(None) => {
                resin.circuits.push(rc);
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

pub fn compile(model: String) -> Resin {
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

        // Solve and build RC
        solve(ctl, rc.clone(), &mut resin);
    }

    // Return the compiled Resin program
    return resin;
}
