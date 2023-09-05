use crate::reactive_circuit::ReactiveCircuit;
use clap::Parser;
use clingo::{control, Control, Model, ModelType, Part, ShowType, SolveMode};
use itertools::Itertools;
use regex::Regex;
use std::panic;
use std::str::FromStr;

enum ResinType {
    Number,
    Probability,
    Density,
}

impl FromStr for ResinType {
    type Err = ();

    fn from_str(input: &str) -> Result<ResinType, Self::Err> {
        match input {
            "Number" => Ok(ResinType::Number),
            "Probability" => Ok(ResinType::Probability),
            "Density" => Ok(ResinType::Density),
            _ => Err(()),
        }
    }
}

pub struct Signal {
    name: String,
    channel: String,
    message_type: ResinType,
}

pub struct Clause {
    head: String,
    probability: f64,
    body: Vec<String>,
}
pub struct Source {
    name: String,
    channel: String,
    message_type: ResinType,
}

impl Source {
    fn to_asp(&self) -> String {
        let asp = format!("{{{}}}.", self.name);
        asp
    }
}

pub struct Target {
    name: String,
    channel: String,
    message_type: ResinType,
}

impl Target {
    fn to_asp(&self) -> String {
        let asp = format!(":- not {}.", self.name);
        asp
    }
}

impl Clause {
    fn to_asp(&self) -> String {
        let mut asp;

        if self.probability != 1.0 {
            asp = format!("{{{}}}", self.head)
        } else {
            asp = format!("{}", self.head);
        }

        if self.body.len() > 0 {
            asp += &format!(" :- {}", self.body[0]);
            for literal in &self.body[1..] {
                asp += &format!(", {}", literal);
            }
        }

        asp += ".";
        asp
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The Resin source to apply
    #[arg(short, long)]
    pub source: String,
}

fn solve(ctl: Control) {
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
        
                for symbol in atoms {
                    print!(" {}", symbol);
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

pub fn parse(model: String) -> Vec<ReactiveCircuit> {
    panic::set_hook(Box::new(|_info| {}));

    // Individual language elements and named groups
    let atom = r"(?<atom>\w+)".to_string();
    let literal = r"(?<literal>(?:not\s+)?\w+)".to_string();
    let probability = r"Probability\((?<probability>[01][.]\d+)\)".to_string();
    let body = r"(?<body>.+)".to_string();
    let topic = r#""(?<topic>(?:\/\w+)+)""#.to_string();
    let dtype = r"(?<dtype>Probability|Density|Number)".to_string();

    // Regular expressions for complete Resin statements
    let literal_regex = Regex::new(&format!(r"(?:\s+and\s+)?{}", literal)).unwrap();
    let clause_regex = Regex::new(&format!(
        r"{}(\s+<-\s+{})?(\s+if\s+{})?\.",
        atom, probability, body
    ))
    .unwrap();
    let source_regex = Regex::new(&format!(
        r#"{}\s+<-\s+source\({},\s+{}\)\."#,
        atom, topic, dtype
    ))
    .unwrap();
    let target_regex = Regex::new(&format!(r#"{}\s+->\s+target\({}\)\."#, atom, topic)).unwrap();

    // Parse Resin source line by line into appropriate data structures
    let mut program = "".to_string();
    let mut targets = vec![];
    for line in model.lines() {
        if source_regex.is_match(line) {
            let Some(captures) = source_regex.captures(line) else { panic!() };

            let source = Source {
                name: captures["atom"].to_string(),
                channel: captures["topic"].to_string(),
                message_type: captures["dtype"].to_string().parse().unwrap(),
            };
            program += &source.to_asp();
            program += "\n";
        } else if target_regex.is_match(line) {
            let Some(captures) = target_regex.captures(line) else { panic!() };

            let target = Target {
                name: captures["atom"].to_string(),
                channel: captures["topic"].to_string(),
                message_type: ResinType::Probability,
            };
            targets.push(target.to_asp());
        } else if clause_regex.is_match(line) {
            let Some(captures) = clause_regex.captures(line) else { panic!() };

            let mut bod = "".to_string();
            match panic::catch_unwind(|| &captures["body"]) {
                Ok(b) => bod += b,
                _ => (),
            }

            let mut atoms = vec![];
            for (_, [atom]) in literal_regex.captures_iter(&bod).map(|c| c.extract()) {
                atoms.push(atom.to_string());
            }

            let mut prob = "1.0".to_string();
            match panic::catch_unwind(|| &captures["probability"]) {
                Ok(p) => prob = p.to_string(),
                _ => (),
            }

            let clause = Clause {
                head: captures["atom"].to_string(),
                probability: prob.parse().unwrap(),
                body: atoms,
            };

            program += &clause.to_asp();
            program += "\n";
        }
    }

    // Pass data to Clingo and obtain stable models
    for target in targets {
        let target_program = program.clone() + &target;
        println!("\n{}\n", &target_program);

        let mut ctl =
            control(vec!["--models=0".to_string()]).expect("Failed creating clingo_control.");
        ctl.add("base", &[], &target_program)
            .expect("Failed to add a logic program.");

        // ground the base part
        let part = Part::new("base", vec![]).unwrap();
        let parts = vec![part];
        ctl.ground(&parts)
            .expect("Failed to ground a logic program.");

        // solve
        solve(ctl);
    }

    let _ = panic::take_hook();

    // Create a Reactive Circuits for each target signal
    return Vec::new();
}
