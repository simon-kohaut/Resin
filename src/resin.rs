use clap::Parser;
use regex::Regex;
use std::str::FromStr;

use crate::reactive_circuit::ReactiveCircuit;

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
    message_type: ResinType,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The Resin source to apply
    #[arg(short, long)]
    pub source: String,
}

pub fn parse(model: String) -> Vec<ReactiveCircuit> {
    // Regular expressions for different parts of Resin
    let atom = r"(?<atom>\w+)".to_string();
    let probability = r"Probability\((?<probability>[01][.]\d+)\)".to_string();
    let body = r"if\s+(?<body>)\.".to_string();
    let topic = r#""(?<topic>\/[a-zA-Z]+(?:_[a-zA-Z]+)*)""#.to_string();
    let dtype = r"(?<dtype>Probability|Density|Number)".to_string();
    let number = r"(?:[!<>=]*\s+\d+(?:\.d+)?)?)".to_string();
    let lambda = r"(?:\s+[!<>=]+\s+\d+\.\d+".to_string();

    let assignment_regex = Regex::new(&format!(r"{}\s+<-\s+{}\.", atom, probability)).unwrap();
    let conditional_probability_regex =
        Regex::new(&format!(r"{}\s+<-\s+{}\s+{}\.", atom, probability, body)).unwrap();
    let source_regex =
        Regex::new(&format!(r#"{}\s+<-\s+source\({},\s+{}\)\."#, atom, topic, dtype)).unwrap();
    let target_regex = Regex::new(&format!(r#"{}\s+->\s+target\({}\)\."#, atom, topic)).unwrap();
    let clause_regex = Regex::new(&format!(r"{}\s+{}\.", atom, body)).unwrap();
    let literal_regex= Regex::new(&format!(r"(?:and\s+)?(?<atom>(?:not\s+))?\w+")).unwrap();

    // Parse Resin source line by line into appropriate data structures
    for line in model.lines() {
        if source_regex.is_match(line) {
            let Some(captures) = source_regex.captures(line) else { panic!() };
            println!(
                "Source {{ Atom: {} | Topic: {} | Type: {} }}",
                &captures["atom"], &captures["topic"], &captures["dtype"]
            );
        } else if target_regex.is_match(line) {
            let Some(captures) = target_regex.captures(line) else { panic!() };
            println!(
                "Target {{ Atom: {} | Topic: {} }}",
                &captures["atom"], &captures["topic"]
            );
        } else if assignment_regex.is_match(line) {
            let Some(captures) = assignment_regex.captures(line) else { panic!() };
            println!(
                "Prob. assignment {{ Atom: {} | Probability: {:?} }}",
                &captures["atom"], &captures["probability"]
            );
        } else if clause_regex.is_match(line) {
            let Some(captures) = clause_regex.captures(line) else { panic!() };
            let mut atoms = vec![];
            for (_, [atom]) in literal_regex
                .captures_iter(&captures["body"])
                .map(|c| c.extract())
            {
                atoms.push(atom);
            }
            println!(
                "Clause {{ Head: {} | Body: {:?} }}",
                &captures["atom"], &atoms
            );
        }
    }

    // Pass data to Clingo and obtain stable models

    // Create a Reactive Circuits for each target signal
    return Vec::new();
}
