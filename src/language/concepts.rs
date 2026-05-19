use std::panic;
use std::str::FromStr;

use regex::Regex;

use super::matching::{
    args_of, canonical_comparison_name, get_literals, has_variable_arg,
    parameterized_comparison_predicate, predicate_of, CATEGORICAL_SOURCE_REGEX, CLAUSE_REGEX,
    COMPARISON_LITERAL_REGEX, SOURCE_REGEX, TARGET_REGEX,
};

/// A comparison literal extracted from a clause body, e.g. `distance(hospital) < 20.0`.
///
/// For **ground** comparisons (`is_variable = false`) `canonical_name` is the flat leaf
/// name used both in the emitted ASP body and as the circuit leaf name,
/// e.g. `"distance_hospital_lt_20"`.
///
/// For **variable** comparisons (`is_variable = true`, e.g. `distance(T) > 100`)
/// `canonical_name` is the parameterized form Clingo can ground,
/// e.g. `"resin_distance_gt_100(T)"`.  Leaf names are produced by `ground_for`.
#[derive(Clone, Debug)]
pub struct ComparisonLiteral {
    pub source_atom: String,
    pub op: char,
    pub threshold: f64,
    pub canonical_name: String,
    /// `true` when `source_atom` contains a Datalog variable argument (uppercase first char).
    pub is_variable: bool,
}

impl ComparisonLiteral {
    /// Returns `true` when the "positive" leaf should carry P(X > threshold),
    /// i.e. the operator is `>`.
    pub fn is_upper_tail(&self) -> bool {
        self.op == '>'
    }

    /// For a variable comparison, returns a fully-grounded `ComparisonLiteral` for
    /// `source_name` (e.g. `"distance(hospital, airport)"`), or `None` when:
    /// - `self` is not a variable comparison,
    /// - the predicate names differ, or
    /// - a constant argument position in the template doesn't match the source.
    pub fn ground_for(&self, source_name: &str) -> Option<ComparisonLiteral> {
        if !self.is_variable {
            return None;
        }
        if predicate_of(&self.source_atom) != predicate_of(source_name) {
            return None;
        }
        // For mixed-arg templates like `distance(A, hub, B)` ensure every constant
        // argument position matches the corresponding source argument.
        if let (Some(tmpl_args), Some(src_args)) =
            (args_of(&self.source_atom), args_of(source_name))
        {
            if tmpl_args.len() != src_args.len() {
                return None;
            }
            for (t, s) in tmpl_args.iter().zip(src_args.iter()) {
                if !t.starts_with(|c: char| c.is_uppercase()) && t != s {
                    return None;
                }
            }
        }
        Some(ComparisonLiteral {
            source_atom: source_name.to_string(),
            op: self.op,
            threshold: self.threshold,
            canonical_name: canonical_comparison_name(source_name, self.op, self.threshold),
            is_variable: false,
        })
    }
}

/// Scans `body` for inline comparison literals (e.g. `distance(hospital) < 20.0`),
/// replaces each one with its canonical name in the text, and returns the full
/// list of parsed body literals alongside the extracted `ComparisonLiteral`s.
fn process_body(body: &str) -> (Vec<String>, Vec<ComparisonLiteral>) {
    let mut comparison_literals: Vec<ComparisonLiteral> = Vec::new();
    let mut processed = body.to_string();
    let mut offset: i64 = 0;

    for caps in COMPARISON_LITERAL_REGEX.captures_iter(body) {
        let m = caps.get(0).unwrap();
        let source_atom = caps["comp_atom"].to_string();
        let op = caps["comp_op"].chars().next().unwrap();
        let threshold: f64 = caps["comp_threshold"].parse().unwrap();
        let is_variable = has_variable_arg(&source_atom);

        // Variable comparisons: emit a parameterized atom Clingo can ground,
        // e.g. `distance(T) > 100` → `resin_distance_gt_100(T)`.
        // Ground comparisons: flat canonical name as before.
        let canonical = if is_variable {
            let pred = predicate_of(&source_atom);
            // Preserve all args (variables stay as variables, constants stay as constants)
            // so Clingo can ground the parameterized atom correctly, e.g.
            // `distance(A, hub, B) > 100` → `resin_distance_gt_100(A, hub, B)`.
            let args_str = args_of(&source_atom)
                .map(|a| a.join(", "))
                .unwrap_or_default();
            format!(
                "{}({})",
                parameterized_comparison_predicate(pred, op, threshold),
                args_str
            )
        } else {
            canonical_comparison_name(&source_atom, op, threshold)
        };

        comparison_literals.push(ComparisonLiteral {
            source_atom,
            op,
            threshold,
            canonical_name: canonical.clone(),
            is_variable,
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

/// A parsed Resin rule, e.g. `unsafe(a, b) <- P(0.65) if close(a, b) and heavy(a).`
///
/// The `head` atom is the conclusion; `probability` carries the `P(…)` weight when
/// present; `body` holds all body literals after comparison literals have been
/// replaced by their canonical names; `comparison_literals` holds the extracted
/// comparisons; `code` is the original source text.
pub struct Clause {
    pub head: String,
    pub probability: Option<f64>,
    /// Regular atom literals (comparison literals are replaced by their canonical names).
    pub body: Vec<String>,
    /// Comparison literals extracted from the body, keyed for compiler use.
    pub comparison_literals: Vec<ComparisonLiteral>,
    pub code: String,
}

/// A declared Resin input source, e.g. `distance(hospital) <- source("/dist/hospital", Density).`
///
/// `name` is the atom that will appear in clause bodies; `channel` is the IPC
/// topic string; `message_type` determines how incoming values are converted into
/// leaf probabilities.
pub struct Source {
    pub name: String,
    pub channel: String,
    pub message_type: ResinType,
}

/// A declared Resin output target, e.g. `safe -> target("/safety").`
///
/// At compile time, the target atom is removed from the DNF and the remaining
/// formula is used to build the reactive circuit.  `message_type` is always
/// `Probability`.
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
    /// A categorical distribution: a probability per class, summing to 1.
    /// Declared with `{cls0, cls1, ..} <- source(channel, Categorical).`
    Categorical,
}

/// A declared Resin categorical source, e.g. `{dog, cat, horse} <- source("/cls", Categorical).`
///
/// Each category atom gets its own positive-only circuit leaf (no complementary leaf).
/// The ASP constraint `1 { dog ; cat ; horse } 1` enforces mutual exclusivity, so
/// each stable model carries exactly one active category.  Because the negative literals
/// (`-dog`, `-cat`, ...) have no corresponding leaves, `circuit_from_dnf` skips them
/// automatically, and the WMC of each product collapses to just the active category's
/// probability — giving the exact categorical sum-of-products.
#[derive(Clone)]
pub struct CategoricalSource {
    pub categories: Vec<String>,
    pub channel: String,
}

impl FromStr for CategoricalSource {
    type Err = ();

    fn from_str(input: &str) -> Result<CategoricalSource, Self::Err> {
        let caps = CATEGORICAL_SOURCE_REGEX.captures(input).ok_or(())?;
        let categories = caps["categories"]
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        Ok(CategoricalSource {
            categories,
            channel: caps["topic"].to_owned(),
        })
    }
}

impl Clause {
    /// Renders this clause as an ASP choice rule (when probabilistic) or a
    /// deterministic rule, terminated with `.\n`.
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

    /// Returns a new `Clause` with every occurrence of `variable` (as a regex
    /// pattern) replaced by `instance` in the source text, then re-parsed.
    pub fn substitute(&self, variable: String, instance: String) -> Clause {
        let regex = Regex::new(&variable).unwrap();
        let substituted = regex.replace_all(&self.code, instance);

        substituted.parse().unwrap()
    }
}

impl Source {
    /// Renders this source as an ASP choice atom `{name}.\n`.
    pub fn to_asp(&self) -> String {
        let asp = format!("{{{}}}.\n", self.name);
        asp
    }
}

impl Target {
    /// Renders this target as an ASP integrity constraint `:- not name.\n`,
    /// which forces Clingo to only consider models where `name` holds.
    pub fn to_asp(&self) -> String {
        let asp = format!(":- not {}.\n", self.name);
        asp
    }
}

impl FromStr for Clause {
    type Err = ();

    /// Parses a Resin rule string into a `Clause`.
    ///
    /// Accepts both fact form (`head.`) and rule form
    /// (`head <- P(p) if body.`).  Returns `Err(())` if the input does not
    /// match the clause grammar.
    fn from_str(input: &str) -> Result<Clause, Self::Err> {
        if CLAUSE_REGEX.is_match(input) {
            let Some(captures) = CLAUSE_REGEX.captures(input) else {
                panic!()
            };

            panic::set_hook(Box::new(|_info| {}));
            let mut body = "".to_string();
            if let Ok(capture) = panic::catch_unwind(|| &captures["body"]) {
                body += capture;
            }
            let (literals, comparison_literals) = process_body(&body);

            let mut probability = None;
            if let Ok(capture) = panic::catch_unwind(|| &captures["probability"]) {
                probability = Some(capture.to_string().parse().unwrap());
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

    /// Parses a Resin source declaration such as
    /// `distance(hospital) <- source("/dist/hospital", Density).`
    /// Returns `Err(())` if the input does not match the source grammar.
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

    /// Parses a Resin target declaration such as `safe -> target("/safety").`
    /// Returns `Err(())` if the input does not match the target grammar.
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
            "Categorical" => Ok(ResinType::Categorical),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resin_type_parsing() {
        assert!(matches!(
            "Probability".parse::<ResinType>().unwrap(),
            ResinType::Probability
        ));
        assert!(matches!(
            "Density".parse::<ResinType>().unwrap(),
            ResinType::Density
        ));
        assert!(matches!(
            "Number".parse::<ResinType>().unwrap(),
            ResinType::Number
        ));
        assert!(matches!(
            "Boolean".parse::<ResinType>().unwrap(),
            ResinType::Boolean
        ));
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
        let lt_comp = clause
            .comparison_literals
            .iter()
            .find(|c| c.op == '<')
            .unwrap();
        let gt_comp = clause
            .comparison_literals
            .iter()
            .find(|c| c.op == '>')
            .unwrap();

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
        assert_eq!(
            clause.comparison_literals[0].source_atom,
            "distance(hospital)"
        );
        // "active" is a regular literal
        assert!(clause.body.contains(&"active".to_string()));
        // Canonical comparison name is also in body
        assert!(clause
            .body
            .contains(&clause.comparison_literals[0].canonical_name));
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

    // -----------------------------------------------------------------------
    // Variable comparison literal tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_variable_comparison_single_var() {
        let code = "safety_distance(T) if critical_infrastructure(T) and distance(T) > 100.";
        let clause: Clause = code.parse().unwrap();

        assert_eq!(clause.comparison_literals.len(), 1);
        let comp = &clause.comparison_literals[0];
        assert!(comp.is_variable);
        assert_eq!(comp.source_atom, "distance(T)");
        assert_eq!(comp.op, '>');
        assert_eq!(comp.threshold, 100.0);
        // Body uses the parameterized, groundable form
        assert_eq!(comp.canonical_name, "resin_distance_gt_100(T)");
        assert!(clause
            .body
            .contains(&"resin_distance_gt_100(T)".to_string()));
    }

    #[test]
    fn test_variable_comparison_multi_var() {
        let code = "close(A, B) if distance(A, B) < 50.";
        let clause: Clause = code.parse().unwrap();

        let comp = &clause.comparison_literals[0];
        assert!(comp.is_variable);
        assert_eq!(comp.source_atom, "distance(A, B)");
        assert_eq!(comp.canonical_name, "resin_distance_lt_50(A, B)");
        assert!(clause
            .body
            .contains(&"resin_distance_lt_50(A, B)".to_string()));
    }

    #[test]
    fn test_ground_comparison_is_not_variable() {
        let code = "safe if distance(hospital) < 20.0.";
        let clause: Clause = code.parse().unwrap();

        let comp = &clause.comparison_literals[0];
        assert!(!comp.is_variable);
        assert_eq!(comp.canonical_name, "distance_hospital_lt_20");
    }

    #[test]
    fn test_ground_for_single_arg() {
        let comp = ComparisonLiteral {
            source_atom: "distance(T)".to_string(),
            op: '>',
            threshold: 100.0,
            canonical_name: "resin_distance_gt_100(T)".to_string(),
            is_variable: true,
        };

        let g = comp.ground_for("distance(hospital)").unwrap();
        assert_eq!(g.source_atom, "distance(hospital)");
        assert_eq!(g.canonical_name, "distance_hospital_gt_100");
        assert!(!g.is_variable);

        // Wrong predicate → None
        assert!(comp.ground_for("speed").is_none());
        // Wrong arity → None
        assert!(comp.ground_for("distance(a, b)").is_none());
    }

    #[test]
    fn test_ground_for_multi_arg() {
        let comp = ComparisonLiteral {
            source_atom: "distance(A, B)".to_string(),
            op: '<',
            threshold: 50.0,
            canonical_name: "resin_distance_lt_50(A, B)".to_string(),
            is_variable: true,
        };

        let g = comp.ground_for("distance(hospital, airport)").unwrap();
        assert_eq!(g.canonical_name, "distance_hospital__airport_lt_50");

        // Wrong arity → None
        assert!(comp.ground_for("distance(hospital)").is_none());
    }

    #[test]
    fn test_ground_for_mixed_constant_arg() {
        // Comparison has a constant in argument position 1 ("hub")
        let comp = ComparisonLiteral {
            source_atom: "distance(A, hub, B)".to_string(),
            op: '>',
            threshold: 100.0,
            canonical_name: "resin_distance_gt_100(A, hub, B)".to_string(),
            is_variable: true,
        };

        // Matching: constant position agrees
        assert!(comp.ground_for("distance(x, hub, y)").is_some());
        // Non-matching: constant position disagrees
        assert!(comp.ground_for("distance(x, other, y)").is_none());
        // Wrong arity → None
        assert!(comp.ground_for("distance(x, y)").is_none());
    }

    #[test]
    fn test_ground_for_returns_none_for_ground_literal() {
        let comp = ComparisonLiteral {
            source_atom: "distance(hospital)".to_string(),
            op: '>',
            threshold: 100.0,
            canonical_name: "distance_hospital_gt_100".to_string(),
            is_variable: false,
        };
        assert!(comp.ground_for("distance(hospital)").is_none());
    }
}
