use clap::Parser;
use regex::Regex;

use crate::reactive_circuit::ReactiveCircuit;

enum ResinType {
    Integer,
    Float,
    Probability,
    Gaussian
}

pub struct Signal {
    name: String,
    message_type: ResinType
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// The Resin source to apply
    #[arg(short, long)]
    pub source: String,
}

pub fn parse(model: String) -> Vec<ReactiveCircuit> {
    let source_re = Regex::new(r#"(?<atom>\w+) <- source\("(?<topic>\/[a-zA-Z]+(?:_[a-zA-Z]+)*)", (?<type>\w+)\)."#).unwrap();
    // let source_re = Regex::new(r#"(?<atom>\w) <- source\("(?<topic>\/[a-zA-Z]+(?:_[a-zA-Z]+)*)", (?<type>\w)\)\."#).unwrap();
    let target_re = Regex::new(r#"(?<atom>\w+) -> target\("(?<topic>\/[a-zA-Z]+(?:_[a-zA-Z]+)*)"\)\."#).unwrap();
    let clause_head_re = Regex::new(r#"(?<atom>\w+) if (?<body>.*?)\."#).unwrap();
    let clause_body_re = Regex::new(r#"(?:and )?(?<atom>\w+)"#).unwrap();

    for line in model.lines() {
        if source_re.is_match(line) {
            let Some(captures) = source_re.captures(line) else { panic!() };
            println!("Source {{ Atom: {} | Topic: {} | Type: {} }}", &captures["atom"], &captures["topic"], &captures["type"]);
        } else if target_re.is_match(line) {
            let Some(captures) = target_re.captures(line) else { panic!() };
            println!("Target {{ Atom {} | Topic: {} }}", &captures["atom"], &captures["topic"]);
        } else if clause_head_re.is_match(line) {
            let Some(captures) = clause_head_re.captures(line) else { panic!() };
            let mut atoms = vec![];
            for (_, [atom]) in clause_body_re.captures_iter(&captures["body"]).map(|c| c.extract()) {
                atoms.push(atom);
            }
            println!("Clause {{ Head: {} | Body: {:?} }}", &captures["atom"], &atoms);
        }
    }

    return Vec::new();
}
