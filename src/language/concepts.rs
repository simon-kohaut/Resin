use std::panic;
use std::str::FromStr;

use regex::Regex;

use super::matching::{
    canonical_comparison_name, get_literals, CLAUSE_REGEX, COMPARISON_LITERAL_REGEX, SOURCE_REGEX,
    TARGET_REGEX,
};

/// A comparison literal extracted from a clause body, e.g. `distance(hospital) < 20.0`.
/// Its `canonical_name` is used as the atom name in ASP and as the leaf name in the circuit.
#[derive(Clone, Debug)]
pub struct ComparisonLiteral {
    pub source_atom: String,
    pub op: char,
    pub threshold: f64,
    pub canonical_name: String,
}

impl ComparisonLiteral {
    /// Returns `true` when the "positive" leaf should carry P(X > threshold),
    /// i.e. the operator is `>`.
    pub fn is_upper_tail(&self) -> bool {
        self.op == '>'
    }
}

/// Extracts comparison literals from a raw body string and returns the
/// processed body (comparisons replaced by their canonical names) together
/// with the list of `ComparisonLiteral`s found.
fn process_body(body: &str) -> (Vec<String>, Vec<ComparisonLiteral>) {
    let mut comparison_literals: Vec<ComparisonLiteral> = Vec::new();
    let mut processed = body.to_string();
    let mut offset: i64 = 0;

    for caps in COMPARISON_LITERAL_REGEX.captures_iter(body) {
        let m = caps.get(0).unwrap();
        let source_atom = caps["comp_atom"].to_string();
        let op = caps["comp_op"].chars().next().unwrap();
        let threshold: f64 = caps["comp_threshold"].parse().unwrap();
        let canonical = canonical_comparison_name(&source_atom, op, threshold);

        comparison_literals.push(ComparisonLiteral {
            source_atom,
            op,
            threshold,
            canonical_name: canonical.clone(),
        });

        let start = (m.start() as i64 + offset) as usize;
        let end = (m.end() as i64 + offset) as usize;
        let old_len = end - start;
        processed.replace_range(start..end, &canonical);
        offset += canonical.len() as i64 - old_len as i64;
    }

    let literals = get_literals(&processed);
    (literals, comparison_literals)
}

pub struct Clause {
    pub head: String,
    pub probability: Option<f64>,
    /// Regular atom literals (comparison literals are replaced by their canonical names).
    pub body: Vec<String>,
    /// Comparison literals extracted from the body, keyed for compiler use.
    pub comparison_literals: Vec<ComparisonLiteral>,
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
    /// A value already in [0, 1] — passed through directly.
    Probability,
    /// A continuous density: CDF/SF evaluated at each comparison threshold.
    Density,
    /// A numeric value: compared against each threshold to produce 0.0 or 1.0.
    Number,
    /// A boolean: `true` → 1.0, `false` → 0.0.
    Boolean,
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
            let (literals, comparison_literals) = process_body(&body);

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
                comparison_literals,
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
            "Probability" => Ok(ResinType::Probability),
            "Density" => Ok(ResinType::Density),
            "Number" => Ok(ResinType::Number),
            "Boolean" => Ok(ResinType::Boolean),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resin_type_parsing() {
        assert!(matches!("Probability".parse::<ResinType>().unwrap(), ResinType::Probability));
        assert!(matches!("Density".parse::<ResinType>().unwrap(), ResinType::Density));
        assert!(matches!("Number".parse::<ResinType>().unwrap(), ResinType::Number));
        assert!(matches!("Boolean".parse::<ResinType>().unwrap(), ResinType::Boolean));
        assert!("Unknown".parse::<ResinType>().is_err());
    }

    #[test]
    fn test_clause_with_comparison_literals() {
        // Clause with a single comparison in the body
        let code = "safe if distance(hospital) < 20.0.";
        let clause: Clause = code.parse().unwrap();

        assert_eq!(clause.head, "safe");
        assert_eq!(clause.comparison_literals.len(), 1);
        let comp = &clause.comparison_literals[0];
        assert_eq!(comp.source_atom, "distance(hospital)");
        assert_eq!(comp.op, '<');
        assert_eq!(comp.threshold, 20.0);
        assert!(!comp.is_upper_tail());
        // The canonical name should appear in the body literals
        assert!(clause.body.contains(&comp.canonical_name));
    }

    #[test]
    fn test_clause_with_multiple_comparison_literals() {
        // Two comparisons on the same atom — different thresholds and directions
        let code = "safe if distance(hospital) < 20.0 and distance(hospital) > 55.0.";
        let clause: Clause = code.parse().unwrap();

        assert_eq!(clause.comparison_literals.len(), 2);
        let lt_comp = clause.comparison_literals.iter().find(|c| c.op == '<').unwrap();
        let gt_comp = clause.comparison_literals.iter().find(|c| c.op == '>').unwrap();

        assert_eq!(lt_comp.threshold, 20.0);
        assert!(!lt_comp.is_upper_tail());
        assert_eq!(gt_comp.threshold, 55.0);
        assert!(gt_comp.is_upper_tail());

        // Both canonical names should be in the body
        assert!(clause.body.contains(&lt_comp.canonical_name));
        assert!(clause.body.contains(&gt_comp.canonical_name));
    }

    #[test]
    fn test_clause_mixed_literals() {
        // One regular atom and one comparison literal in the body
        let code = "at_risk if active and distance(hospital) < 5.0.";
        let clause: Clause = code.parse().unwrap();

        assert_eq!(clause.comparison_literals.len(), 1);
        assert_eq!(clause.comparison_literals[0].source_atom, "distance(hospital)");
        // "active" is a regular literal
        assert!(clause.body.contains(&"active".to_string()));
        // Canonical comparison name is also in body
        assert!(clause.body.contains(&clause.comparison_literals[0].canonical_name));
    }

    #[test]
    fn test_source_with_boolean_type() {
        let code = r#"active <- source("/active", Boolean)."#;
        let source: Source = code.parse().unwrap();
        assert_eq!(source.name, "active");
        assert_eq!(source.channel, "/active");
        assert!(matches!(source.message_type, ResinType::Boolean));
    }

    #[test]
    fn test_source_with_density_type() {
        let code = r#"distance(hospital) <- source("/distance/hospital", Density)."#;
        let source: Source = code.parse().unwrap();
        assert_eq!(source.name, "distance(hospital)");
        assert!(matches!(source.message_type, ResinType::Density));
    }
}
