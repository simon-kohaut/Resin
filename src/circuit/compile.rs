use crate::circuit::mul::Mul;
use crate::circuit::rc::RC;
use crate::language::Resin;
use clap::Parser;
use clingo::{control, Control, ModelType, Part, ShowType, SolveMode};

use std::panic;

use super::category::Category;
use super::leaf::activate_channel;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The Resin source to apply
    #[arg(short, long)]
    pub source: String,
}

fn solve(ctl: Control, rc: &mut RC, resin: &mut Resin) {
    // get a solve handle
    let mut handle = ctl
        .solve(SolveMode::YIELD, &[])
        .expect("Failed retrieving solve handle.");

    // loop over all models
    loop {
        handle.resume().expect("Failed resume on solve handle.");
        match handle.model() {
            Ok(Some(stable_model)) => {
                // Get model type
                let model_type = stable_model.model_type().unwrap();

                let type_string = match model_type {
                    ModelType::StableModel => "Stable model",
                    ModelType::BraveConsequences => "Brave consequences",
                    ModelType::CautiousConsequences => "Cautious consequences",
                };

                // Get running number of model
                let number = stable_model.number().unwrap();

                print!("{} {}:", type_string, number);
                let atoms = stable_model
                    .symbols(ShowType::ATOMS)
                    .expect("Failed to retrieve positive symbols in the model.");

                let complement = stable_model
                    .symbols(ShowType::COMPLEMENT | ShowType::ALL)
                    .expect("Failed to retrieve complementary symbols in the model.");

                let mut mul = Mul::empty_new();
                println!();
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
                            match rc
                                .foliage
                                .clone()
                                .lock()
                                .unwrap()
                                .iter()
                                .position(|leaf| leaf.name == name)
                            {
                                Some(index) => {
                                    mul.mul_index(index);
                                    println!(
                                        "Added source {}",
                                        &rc.foliage.lock().unwrap()[index].name
                                    );
                                }
                                None => {
                                    let category = Category::new(&name);
                                    let index = rc.foliage.lock().unwrap().len();
                                    activate_channel(
                                        rc.foliage.clone(),
                                        index,
                                        &source.channel,
                                        &false,
                                    );
                                    activate_channel(
                                        rc.foliage.clone(),
                                        index + 1,
                                        &source.channel,
                                        &true,
                                    );
                                    rc.grow(category.leafs[0].get_value(), &category.leafs[0].name);
                                    rc.grow(category.leafs[1].get_value(), &category.leafs[1].name);
                                    mul.mul_index(index);

                                    println!("Added source {}", &category.leafs[0].name);
                                }
                            }
                        }
                    }

                    // Check if source atom is in complementary symbols
                    // If it is, add it with an initial probability of 1
                    for symbol in &complement {
                        let name = format!("{}", symbol);
                        if source.name == name {
                            match rc
                                .foliage
                                .clone()
                                .lock()
                                .unwrap()
                                .iter()
                                .position(|leaf| leaf.name == name)
                            {
                                Some(index) => {
                                    mul.mul_index(index);
                                    println!(
                                        "Added source {}",
                                        &rc.foliage.lock().unwrap()[index].name
                                    );
                                }
                                None => {
                                    let category = Category::new(&name);
                                    let index = rc.foliage.lock().unwrap().len();
                                    activate_channel(
                                        rc.foliage.clone(),
                                        index,
                                        &source.channel,
                                        &false,
                                    );
                                    activate_channel(
                                        rc.foliage.clone(),
                                        index + 1,
                                        &source.channel,
                                        &true,
                                    );
                                    rc.grow(category.leafs[0].get_value(), &category.leafs[0].name);
                                    rc.grow(category.leafs[1].get_value(), &category.leafs[1].name);
                                    mul.mul_index(index + 1);

                                    println!("Added source {}", &category.leafs[1].name);
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
                                let index;
                                match rc
                                    .foliage
                                    .clone()
                                    .lock()
                                    .unwrap()
                                    .iter()
                                    .position(|leaf| leaf.name == node_name)
                                {
                                    Some(position) => index = position,
                                    None => {
                                        index = rc.grow(clause.probability.unwrap(), &node_name)
                                    }
                                }
                                mul.mul_index(index);
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
                            let index;
                            match rc
                                .foliage
                                .clone()
                                .lock()
                                .unwrap()
                                .iter()
                                .position(|leaf| leaf.name == node_name)
                            {
                                Some(position) => index = position,
                                None => {
                                    index = rc.grow(1.0 - clause.probability.unwrap(), &node_name)
                                }
                            }
                            mul.mul_index(index);
                            println!("Added {} = {}", node_name, clause.probability.unwrap());
                        }
                    }
                }

                rc.add(mul);
                println!();
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
}

pub fn compile(model: String) -> Resin {
    let mut resin = model.parse::<Resin>().unwrap();

    // Pass data to Clingo and obtain stable models
    for target_index in 0..resin.targets.len() {
        let mut rc = RC::new();

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
        solve(ctl, &mut rc, &mut resin);
        rc.update_dependencies();
        resin.circuits.push(rc);
    }

    // Return the compiled Resin program
    resin
}
