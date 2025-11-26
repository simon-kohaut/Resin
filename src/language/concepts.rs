use std::panic;
use std::str::FromStr;

use regex::Regex;

use super::matching::{get_literals, CLAUSE_REGEX, SOURCE_REGEX, TARGET_REGEX};

pub struct Clause {
    pub head: String,
    pub probability: Option<f64>,
    pub body: Vec<String>,
    pub code: String,
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

impl Clause {
    pub fn to_asp(&self) -> String {
        let mut asp;

        if self.probability.is_some() {
            asp = format!("{{{}}}", self.head)
        } else {
            asp = self.head.to_string();
        }

        if !self.body.is_empty() {
            asp += &format!(" :- {}", self.body[0]);
            for literal in &self.body[1..] {
                asp += &format!(", {}", literal);
            }
        }

        asp += ".\n";
        asp
    }

    pub fn substitute(&self, variable: String, instance: String) -> Clause {
        let regex = Regex::new(&variable).unwrap();
        let substituted = regex.replace_all(&self.code, instance);

        substituted.parse().unwrap()
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

impl FromStr for Clause {
    type Err = ();

    fn from_str(input: &str) -> Result<Clause, Self::Err> {
        if CLAUSE_REGEX.is_match(input) {
            let Some(captures) = CLAUSE_REGEX.captures(input) else {
                panic!()
            };

            panic::set_hook(Box::new(|_info| {}));
            let mut body = "".to_string();
            match panic::catch_unwind(|| &captures["body"]) {
                Ok(capture) => body += capture,
                _ => (),
            }
            let literals = get_literals(&body);

            let mut probability = None;
            match panic::catch_unwind(|| &captures["probability"]) {
                Ok(capture) => probability = Some(capture.to_string().parse().unwrap()),
                _ => (),
            }
            let _ = panic::take_hook();

            let clause = Clause {
                head: captures["atom"].to_string(),
                probability,
                body: literals,
                code: input.to_string(),
            };

            Ok(clause)
        } else {
            Err(())
        }
    }
}

impl FromStr for Source {
    type Err = ();

    fn from_str(input: &str) -> Result<Source, Self::Err> {
        if SOURCE_REGEX.is_match(input) {
            let Some(captures) = SOURCE_REGEX.captures(input) else {
                panic!()
            };

            let source = Source {
                name: captures["atom"].to_string(),
                channel: captures["topic"].to_string(),
                message_type: captures["dtype"].to_string().parse().unwrap(),
            };

            Ok(source)
        } else {
            Err(())
        }
    }
}

impl FromStr for Target {
    type Err = ();

    fn from_str(input: &str) -> Result<Target, Self::Err> {
        if TARGET_REGEX.is_match(input) {
            let Some(captures) = TARGET_REGEX.captures(input) else {
                panic!()
            };

            let target = Target {
                name: captures["atom"].to_string(),
                channel: captures["topic"].to_string(),
                message_type: ResinType::Probability,
            };

            Ok(target)
        } else {
            Err(())
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
