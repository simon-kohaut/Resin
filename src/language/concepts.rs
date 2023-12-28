use std::panic;
use std::str::FromStr;

use rclrs::RclrsError;
use regex::Regex;

use crate::channels::manager::Manager;
use crate::circuit::category::Category;
use crate::circuit::reactive::ReactiveCircuit;

use super::matching::{get_literals, CLAUSE_REGEX, SOURCE_REGEX, TARGET_REGEX};

pub struct Resin {
    pub circuits: Vec<ReactiveCircuit>,
    pub clauses: Vec<Clause>,
    pub sources: Vec<Source>,
    pub targets: Vec<Target>,
    pub manager: Manager,
}

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

    pub fn setup_signals(&mut self) -> Result<(), RclrsError> {
        // Create all source channels and parameter leafs
        for source in &self.sources {
            let category = Category::new(&source.name, 0.0);

            let index = self.manager.create_leaf(
                &category.leafs[0].name,
                category.leafs[0].get_value(),
                0.0,
            );
            self.manager.read(index as u16, &source.channel, false)?;

            let index = self.manager.create_leaf(
                &category.leafs[1].name,
                category.leafs[1].get_value(),
                0.0,
            );
            self.manager.read(index as u16 + 1, &source.channel, true)?;
        }

        for clause in &self.clauses {
            // Clauses that are deterministic do not need to be included in model
            if clause.probability.is_none() {
                continue;
            }

            let category = Category::new(&clause.head, clause.probability.unwrap());

            self.manager
                .create_leaf(&category.leafs[0].name, category.leafs[0].get_value(), 0.0);
            self.manager
                .create_leaf(&category.leafs[1].name, category.leafs[1].get_value(), 0.0);
        }

        Ok(())
    }
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

impl FromStr for Resin {
    type Err = ();

    fn from_str(input: &str) -> Result<Resin, Self::Err> {
        let mut resin = Resin {
            circuits: vec![],
            clauses: vec![],
            sources: vec![],
            targets: vec![],
            manager: Manager::new(),
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
            let Some(captures) = SOURCE_REGEX.captures(input) else { panic!() };

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
            let Some(captures) = TARGET_REGEX.captures(input) else { panic!() };

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

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_clauses() {
        let code = "test.";
        let clause: Clause = code.parse().expect("Parse clause failed!");
        assert!(clause.body.is_empty());
        assert_eq!(clause.code, code);
        assert_eq!(clause.head, "test");
        assert!(clause.probability.is_none());

        let code = "pilot(ben).";
        let clause: Clause = code.parse().expect("Parse clause failed!");
        assert!(clause.body.is_empty());
        assert_eq!(clause.code, code);
        assert_eq!(clause.head, "pilot(ben)");
        assert!(clause.probability.is_none());

        let code = "heavy(drone_1) <- P(0.8).";
        let clause: Clause = code.parse().expect("Parse clause failed!");
        assert!(clause.body.is_empty());
        assert_eq!(clause.code, code);
        assert_eq!(clause.head, "heavy(drone_1)");
        assert_eq!(clause.probability.unwrap(), 0.8);

        let code =
            "unsafe(drone_1, drone_2) <- P(0.65) if close(drone_1, drone_2) and heavy(drone_1).";
        let clause: Clause = code.parse().expect("Parse clause failed!");
        assert_eq!(clause.code, code);
        assert_eq!(clause.head, "unsafe(drone_1, drone_2)");
        assert_eq!(clause.probability.unwrap(), 0.65);
        assert_eq!(
            clause.body,
            vec!["close(drone_1, drone_2)", "heavy(drone_1)"]
        );
    }

    fn test_sources() {}

    fn test_targets() {}
}
