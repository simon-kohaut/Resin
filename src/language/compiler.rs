use crate::circuit::reactive::ReactiveCircuit;
use crate::circuit::category::Category;
use crate::language::Resin;

use clap::Parser;
use clingo::{control, Symbol, Control, ModelType, Part, ShowType, SolveMode};
use itertools::Itertools;
use rclrs::RclrsError;

use std::panic;

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

                println!();
                println!(
                    "Positive: {:?}",
                    atoms
                        .iter()
                        .map(|symbol| format!("{}", symbol))
                        .collect::<Vec<String>>()
                );
                println!(
                    "Negative: {:?}",
                    complement
                        .iter()
                        .map(|symbol| format!("{}", symbol))
                        .collect::<Vec<String>>()
                );

                let mut product = vec![];
                for source in &resin.sources {
                    // Check if source atom is in symbols
                    // If it is, add it with an initial probability of 0
                    for symbol in &atoms {
                        let name = format!("{}", symbol);
                        if source.name == name {
                            let position = resin.manager.get_names()
                                .iter()
                                .position(|leaf_name| *leaf_name == name);
                            match position
                            {
                                Some(index) => {
                                    product.push(index as u16);
                                    println!(
                                        "Added source {}",
                                        &resin.manager.foliage.lock().unwrap()[index].name
                                    );
                                }
                                None => {
                                    let category = Category::new(&name, 0.0);
                                    let index = resin.manager.foliage.lock().unwrap().len();

                                    resin.manager.read(index as u16, &source.channel, false)?;
                                    resin.manager.read(index as u16 + 1, &source.channel, true)?;

                                    resin.manager.create_leaf(&category.leafs[0].name, category.leafs[0].get_value(), 0.0);
                                    resin.manager.create_leaf(&category.leafs[1].name, category.leafs[1].get_value(), 0.0);
                                    
                                    product.push(index as u16);
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
                            let position = resin.manager.get_names()
                                .iter()
                                .position(|leaf_name| *leaf_name == name);
                            match position
                            {
                                Some(index) => {
                                    product.push(index as u16);
                                    println!(
                                        "Added source {}",
                                        &resin.manager.foliage.lock().unwrap()[index].name
                                    );
                                }
                                None => {
                                    let category = Category::new(&name, 0.0);
                                    let index = resin.manager.foliage.lock().unwrap().len();

                                    resin.manager.read(index as u16, &source.channel, false)?;
                                    resin.manager.read(index as u16 + 1, &source.channel, true)?;

                                    resin.manager.create_leaf(&category.leafs[0].name, category.leafs[0].get_value(), 0.0);
                                    resin.manager.create_leaf(&category.leafs[1].name, category.leafs[1].get_value(), 0.0);
                                    
                                    product.push(index as u16 + 1);
                                    println!("Added source {}", &category.leafs[1].name);
                                }
                            }
                        }
                    }
                }

                for clause in &resin.clauses {
                    // Clauses that are deterministic do not need to be included in model
                    if clause.probability.is_none() {
                        println!("Prob was none for {}", clause.head);
                        continue;
                    }

                    // Check if clause atom is in symbols
                    // If it is, check if conditions are met and add its conditional probability
                    for symbol in &atoms {
                        let name = format!("{}", symbol);
                        let node_name = format!("P({} | {})", name, clause.body.join(", "));

                        println!("{} vs {} is {}", clause.head, name, clause.head == name);
                        if clause.head == name {
                            // If the conditions in this clause are violated,
                            // We do not add the conditional probability
                            // TODO: This requires noisy or if multiple conditions are met at once
                            // let mut conditions_met = true;
                            // for condition in &clause.body {
                            //     for complementary in &complement {
                            //         if condition == complementary.name().unwrap() {
                            //             conditions_met = false;
                            //             break;
                            //         }
                            //     }
                            // }

                            // if conditions_met {
                            let index;
                            let position = resin.manager.get_names()
                                .iter()
                                .position(|leaf_name| *leaf_name == name);
                            match position
                            {
                                Some(position) => index = position,
                                None => {
                                    index = resin.manager.create_leaf(&node_name, clause.probability.unwrap(), 0.0) as usize;
                                }
                            }
                            product.push(index as u16);
                            println!("Added {} = {}", node_name, clause.probability.unwrap());
                            // }
                        }
                    }

                    // Check if source atom is in complementary symbols
                    // If it is, add it with an initial probability of 1
                    for symbol in &complement {
                        let name = format!("¬{}", symbol);
                        let node_name = if clause.body.is_empty() {
                            format!("P({})", clause.head)
                        } else {
                            format!("P({} | {})", clause.head, clause.body.join(", "))
                        };
                        if clause.head == name[2..] {
                            let index;
                            let position = resin.manager.get_names()
                                .iter()
                                .position(|leaf_name| *leaf_name == name);
                            match position
                            {
                                Some(position) => index = position,
                                None => {
                                    index = resin.manager.create_leaf(&node_name, 1.0 - clause.probability.unwrap(), 0.0) as usize;
                                }
                            }
                            product.push(index as u16);
                        }
                    }
                }

                rc.products.push((product, None));
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

    Ok(())
}

pub fn compile(model: String) -> Result<Resin, RclrsError> {
    let mut resin = model.parse::<Resin>().unwrap();

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

        // Solve and build RC
        solve(ctl, &mut rc, &mut resin)?;
        rc.set_dependencies(&mut resin.manager, None, vec![]);
        resin.circuits.push(rc);
    }

    // Return the compiled Resin program
    Ok(resin)
}



#[cfg(test)]
mod tests {
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

}
