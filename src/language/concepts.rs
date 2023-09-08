use std::collections::HashMap;
use std::panic;
use std::str::FromStr;

use crate::circuit::SharedLeaf;

use super::super::circuit::ReactiveCircuit;
use super::matching::{CLAUSE_REGEX, LITERAL_REGEX, SOURCE_REGEX, TARGET_REGEX};

pub struct Resin {
    pub circuits: Vec<ReactiveCircuit>,
    pub clauses: Vec<Clause>,
    pub sources: Vec<Source>,
    pub targets: Vec<Target>,
    pub leafs: HashMap<String, SharedLeaf>,
}

pub struct Clause {
    pub head: String,
    pub probability: f64,
    pub body: Vec<String>,
}

pub struct Source {
    pub name: String,
    pub channel: String,
    pub message_type: ResinType,
}

pub struct Target {
    pub name: String,
    pub channel: String,
    pub message_type: ResinType,
}
pub enum ResinType {
    Number,
    Probability,
    Density,
}

impl Resin {
    pub fn to_asp(&self, target_index: usize) -> String {
        let mut asp = "".to_string();

        for source in &self.sources {
            asp.push_str(&source.to_asp());
        }

        for clause in &self.clauses {
            asp.push_str(&clause.to_asp());
        }

        asp.push_str(&self.targets[target_index].to_asp());
        asp
    }
}

impl Clause {
    pub fn to_asp(&self) -> String {
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

        asp += ".\n";
        asp
    }
}

impl Source {
    pub fn to_asp(&self) -> String {
        let asp = format!("{{{}}}.\n", self.name);
        asp
    }
}

impl Target {
    pub fn to_asp(&self) -> String {
        let asp = format!(":- not {}.\n", self.name);
        asp
    }
}

impl FromStr for Resin {
    type Err = ();

    fn from_str(input: &str) -> Result<Resin, Self::Err> {
        let mut resin = Resin {
            circuits: vec![],
            clauses: vec![],
            sources: vec![],
            targets: vec![],
            leafs: HashMap::new(),
        };

        // Parse Resin source line by line into appropriate data structures
        for line in input.lines() {
            let source = line.parse::<Source>();
            if source.is_ok() {
                resin.sources.push(source.unwrap());
                continue;
            }

            let target = line.parse::<Target>();
            if target.is_ok() {
                resin.targets.push(target.unwrap());
                continue;
            }

            let clause = line.parse::<Clause>();
            if clause.is_ok() {
                resin.clauses.push(clause.unwrap());
                continue;
            }
        }

        Ok(resin)
    }
}

impl FromStr for Clause {
    type Err = ();

    fn from_str(input: &str) -> Result<Clause, Self::Err> {
        if CLAUSE_REGEX.is_match(input) {
            let Some(captures) = CLAUSE_REGEX.captures(input) else { panic!() };

            panic::set_hook(Box::new(|_info| {}));
            let mut body = "".to_string();
            match panic::catch_unwind(|| &captures["body"]) {
                Ok(capture) => body += capture,
                _ => (),
            }

            let mut probability = "1.0".to_string();
            match panic::catch_unwind(|| &captures["probability"]) {
                Ok(capture) => probability = capture.to_string(),
                _ => (),
            }
            let _ = panic::take_hook();

            let mut literals = vec![];
            for (_, [literal]) in LITERAL_REGEX.captures_iter(&body).map(|c| c.extract()) {
                literals.push(literal.to_string());
            }

            let clause = Clause {
                head: captures["atom"].to_string(),
                probability: probability.parse().unwrap(),
                body: literals,
            };

            return Ok(clause);
        } else {
            return Err(());
        }
    }
}

impl FromStr for Source {
    type Err = ();

    fn from_str(input: &str) -> Result<Source, Self::Err> {
        if SOURCE_REGEX.is_match(input) {
            let Some(captures) = SOURCE_REGEX.captures(input) else { panic!() };

            let source = Source {
                name: captures["atom"].to_string(),
                channel: captures["topic"].to_string(),
                message_type: captures["dtype"].to_string().parse().unwrap(),
            };

            return Ok(source);
        } else {
            return Err(());
        }
    }
}

impl FromStr for Target {
    type Err = ();

    fn from_str(input: &str) -> Result<Target, Self::Err> {
        if TARGET_REGEX.is_match(input) {
            let Some(captures) = TARGET_REGEX.captures(input) else { panic!() };

            let target = Target {
                name: captures["atom"].to_string(),
                channel: captures["topic"].to_string(),
                message_type: ResinType::Probability,
            };

            return Ok(target);
        } else {
            return Err(());
        }
    }
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
