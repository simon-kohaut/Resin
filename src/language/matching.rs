use lazy_static::lazy_static;
use regex::Regex;

// Individual language elements and named groups
const ATOM_PATTERN: &str = r"(?<atom>\w+(\([\w\s,]+\))?)";
const LITERAL_PATTERN: &str = r"(?<literal>(not\s+)?\w+(\([\w\s,]+\))?)";
const PROBABILITY_PATTERN: &str = r"P\((?<probability>[01][.]\d+)\)";
const BODY_PATTERN: &str = r"(?<body>.+)";
const TOPIC_PATTERN: &str = r#""(?<topic>(?:\/\w+)+)""#;
const DTYPE_PATTERN: &str = r"(?<dtype>Probability|Density|Number|Boolean)";
const VARIABLE_LIST_PATTERN: &str = r"((?:\()(?:(?:,\s+)?\w+)+(?:\)))";
const VARIABLE_PATTERN: &str = r"((?:(,\s+)?)(?<variable>[A-Z]))";
// Matches comparison literals in clause bodies, e.g. `distance(hospital) < 20.0`
const COMPARISON_PATTERN: &str =
    r"(?<comp_atom>\w+(?:\([\w\s,]+\))?)\s+(?<comp_op>[<>])\s+(?<comp_threshold>[+-]?\d+(?:\.\d+)?)";

// Regular expressions for complete Resin statements
lazy_static! {
    pub static ref LITERAL_REGEX: Regex = Regex::new(&LITERAL_PATTERN).unwrap();
    pub static ref CLAUSE_REGEX: Regex = Regex::new(&format!(
        r"{}(\s+<-\s+{})?(\s+if\s+{})?\.",
        ATOM_PATTERN, PROBABILITY_PATTERN, BODY_PATTERN
    ))
    .unwrap();
    pub static ref SOURCE_REGEX: Regex = Regex::new(&format!(
        r#"{}\s+<-\s+source\({},\s+{}\)\."#,
        ATOM_PATTERN, TOPIC_PATTERN, DTYPE_PATTERN
    ))
    .unwrap();
    pub static ref TARGET_REGEX: Regex = Regex::new(&format!(
        r#"{}\s+->\s+target\({}\)\."#,
        ATOM_PATTERN, TOPIC_PATTERN
    ))
    .unwrap();
    pub static ref VARIABLE_LIST_REGEX: Regex = Regex::new(VARIABLE_LIST_PATTERN).unwrap();
    pub static ref VARIABLE_REGEX: Regex = Regex::new(VARIABLE_PATTERN).unwrap();
    pub static ref COMPARISON_LITERAL_REGEX: Regex = Regex::new(COMPARISON_PATTERN).unwrap();
    pub static ref AND_KEYWORD_REGEX: Regex = Regex::new(r"\band\b").unwrap();
}

/// Splits a preprocessed (comment-stripped, joined) Resin program string into
/// individual statements.
///
/// A statement ends at a `.` that is **not** followed by an ASCII digit, so
/// decimal thresholds like `20.0` and probabilities like `P(0.9)` are never
/// split mid-token.  The terminating `.` is included in each returned string
/// so the individual line parsers still see a well-formed statement.
pub fn split_statements(input: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = input.chars().collect();
    let len = chars.len();
    let mut i = 0;
    while i < len {
        current.push(chars[i]);
        if chars[i] == '.' {
            let next = chars.get(i + 1).copied().unwrap_or(' ');
            if !next.is_ascii_digit() {
                let stmt = current.trim().to_string();
                if !stmt.is_empty() {
                    statements.push(stmt);
                }
                current = String::new();
            }
        }
        i += 1;
    }
    statements
}

/// Returns `true` if `atom` has at least one variable argument
/// (an argument whose first character is uppercase), e.g. `"distance(T)"`.
pub fn has_variable_arg(atom: &str) -> bool {
    match atom.find('(') {
        Some(open) => atom[open + 1..atom.len() - 1]
            .split(',')
            .any(|a| a.trim().starts_with(|c: char| c.is_uppercase())),
        None => atom.starts_with(|c: char| c.is_uppercase()),
    }
}

/// Returns the predicate name of an atom, stripping any argument list.
/// E.g. `"distance(hospital)"` → `"distance"`.
pub fn predicate_of(atom: &str) -> &str {
    atom.find('(').map_or(atom, |i| &atom[..i])
}

/// Returns all trimmed arguments of an atom as a vec, e.g.
/// `"distance(A, something, B)"` → `["A", "something", "B"]`.
/// Returns `None` for atoms with no argument list.
pub fn args_of(atom: &str) -> Option<Vec<&str>> {
    let open = atom.find('(')?;
    let close = atom.rfind(')')?;
    Some(atom[open + 1..close].split(',').map(str::trim).collect())
}

/// The ASP predicate name used in the body of rules whose comparison source
/// atom contains a variable.  Keeps the variable groundable by Clingo.
/// E.g. predicate `"distance"`, op `'>'`, threshold `100.0` → `"resin_distance_gt_100"`.
pub fn parameterized_comparison_predicate(predicate: &str, op: char, threshold: f64) -> String {
    let op_str = if op == '<' { "lt" } else { "gt" };
    let t_str = format!("{}", threshold).replace('.', "_");
    format!("resin_{}_{}_{}", predicate, op_str, t_str)
}

/// Produces the auxiliary choice-atom name for the Nth probabilistic clause
/// with a given **ground** head, used to implement independent noisy-OR causes.
/// Args are folded into the name so the result is a flat ASP atom.
/// E.g. head `"close(a, b)"`, index `0` → `"close_a_b_cause_0"`.
pub fn cause_atom_name(head: &str, index: usize) -> String {
    let sanitized: String = head
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    let sanitized = sanitized.trim_matches('_');
    format!("{}_cause_{}", sanitized, index)
}

/// Produces the base predicate name for the auxiliary cause atom of a FOL
/// (variable-head) probabilistic clause.  Only the head predicate is used —
/// not the args — so the result can be extended with the head's arg list to
/// form a parameterized atom that Clingo can ground independently per instance.
/// E.g. head `"heads(C)"`, index `0` → `"heads_cause_0"`.
pub fn cause_atom_base_name(head: &str, index: usize) -> String {
    let pred = predicate_of(head);
    let sanitized: String = pred
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    format!("{}_cause_{}", sanitized.trim_matches('_'), index)
}

/// Produces the canonical atom name for a comparison literal so it can be
/// used as a valid Resin/ASP atom and as a leaf name in the circuit.
/// E.g. `distance(hospital) < 20.0` → `"distance_hospital_lt_20"`.
pub fn canonical_comparison_name(atom: &str, op: char, threshold: f64) -> String {
    let sanitized: String = atom
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect();
    let sanitized = sanitized.trim_matches('_');
    let op_str = if op == '<' { "lt" } else { "gt" };
    let t_str = format!("{}", threshold).replace('.', "_");
    format!("{}_{}_{}", sanitized, op_str, t_str)
}

/// Extracts all atom literals from a body string by stripping the `and`
/// conjunctive keyword and applying `LITERAL_REGEX`.
/// Returns the matching substrings in order.
pub fn get_literals(body: &str) -> Vec<String> {
    let body = AND_KEYWORD_REGEX.replace_all(body, " ");

    LITERAL_REGEX
        .find_iter(&body)
        .map(|m| m.as_str().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_get_literals_and_keyword() {
        // "and" as a conjunction keyword must be stripped.
        assert_eq!(get_literals("foo and bar"), vec!["foo", "bar"]);
        // "and" inside an atom name must NOT be stripped.
        assert_eq!(get_literals("random and command"), vec!["random", "command"]);
        assert_eq!(get_literals("sand_check"), vec!["sand_check"]);
        // Body mixing embedded-"and" atom with the keyword.
        assert_eq!(
            get_literals("random and safety_distance(T)"),
            vec!["random", "safety_distance(T)"]
        );
    }

    #[test]
    fn test_literal() {
        let input = "test";
        let Some(captures) = LITERAL_REGEX.captures(input) else {
            panic!()
        };
        assert_eq!(&captures["literal"], input);

        let input = "test(a)";
        let Some(captures) = LITERAL_REGEX.captures(input) else {
            panic!()
        };
        assert_eq!(&captures["literal"], input);

        let input = "test(a, b)";
        let Some(captures) = LITERAL_REGEX.captures(input) else {
            panic!()
        };
        assert_eq!(&captures["literal"], input);

        let input = "not test(a_1, b, c)";
        let Some(captures) = LITERAL_REGEX.captures(input) else {
            panic!()
        };
        assert_eq!(&captures["literal"], input);
    }

    #[test]
    fn test_canonical_comparison_name() {
        // Parentheses become underscores; trailing _ is trimmed before the op segment
        assert_eq!(
            canonical_comparison_name("distance(hospital)", '<', 20.0),
            "distance_hospital_lt_20"
        );
        assert_eq!(
            canonical_comparison_name("distance(hospital)", '>', 55.0),
            "distance_hospital_gt_55"
        );
        assert_eq!(
            canonical_comparison_name("speed", '<', 10.5),
            "speed_lt_10_5"
        );
        assert_eq!(
            canonical_comparison_name("temperature(room_1)", '>', 22.5),
            "temperature_room_1_gt_22_5"
        );
    }

    #[test]
    fn test_comparison_literal_regex() {
        let input = "distance(hospital) < 20.0";
        let caps = COMPARISON_LITERAL_REGEX.captures(input).unwrap();
        assert_eq!(&caps["comp_atom"], "distance(hospital)");
        assert_eq!(&caps["comp_op"], "<");
        assert_eq!(&caps["comp_threshold"], "20.0");

        let input = "speed > 5";
        let caps = COMPARISON_LITERAL_REGEX.captures(input).unwrap();
        assert_eq!(&caps["comp_atom"], "speed");
        assert_eq!(&caps["comp_op"], ">");
        assert_eq!(&caps["comp_threshold"], "5");

        // Both comparisons in a body should be found
        let body = "distance(hospital) < 20.0 and distance(hospital) > 55.0";
        let matches: Vec<_> = COMPARISON_LITERAL_REGEX.find_iter(body).collect();
        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn test_body() {
        let input = "a if test.";
        let Some(captures) = CLAUSE_REGEX.captures(input) else {
            panic!()
        };
        assert_eq!(&captures["body"], "test");

        let input = "a if test and other.";
        let Some(captures) = CLAUSE_REGEX.captures(input) else {
            panic!()
        };
        assert_eq!(&captures["body"], "test and other");

        let input = "a(X, Y) if test and other.";
        let Some(captures) = CLAUSE_REGEX.captures(input) else {
            panic!()
        };
        assert_eq!(&captures["body"], "test and other");

        let input = "a_b(X, some_thing) <- P(0.4) if test(X) and other(some_thing, C).";
        let Some(captures) = CLAUSE_REGEX.captures(input) else {
            panic!()
        };
        assert_eq!(&captures["atom"], "a_b(X, some_thing)");
        assert_eq!(&captures["body"], "test(X) and other(some_thing, C)");
        assert_eq!(
            get_literals(&captures["body"]),
            vec!["test(X)", "other(some_thing, C)"]
        );
    }

    #[test]
    fn test_cause_atom_name() {
        assert_eq!(cause_atom_name("close(a, b)", 0), "close_a__b_cause_0");
        assert_eq!(cause_atom_name("risky", 0), "risky_cause_0");
        assert_eq!(cause_atom_name("risky", 1), "risky_cause_1");
        assert_eq!(cause_atom_name("unsafe(drone_1)", 2), "unsafe_drone_1_cause_2");
    }

    #[test]
    fn test_has_variable_arg() {
        assert!(has_variable_arg("distance(T)"));
        assert!(has_variable_arg("distance(A, B)"));
        assert!(has_variable_arg("distance(A, hub, B)"));  // mixed constant/variable
        assert!(!has_variable_arg("distance(hospital)"));
        assert!(!has_variable_arg("distance(hospital, airport)"));
        assert!(!has_variable_arg("speed"));               // bare atom, lowercase
        assert!(has_variable_arg("T"));                    // bare variable
    }

    #[test]
    fn test_args_of() {
        assert_eq!(args_of("distance(hospital)"), Some(vec!["hospital"]));
        assert_eq!(
            args_of("distance(hospital, airport)"),
            Some(vec!["hospital", "airport"])
        );
        assert_eq!(
            args_of("distance(A, hub, B)"),
            Some(vec!["A", "hub", "B"])
        );
        assert_eq!(args_of("speed"), None);
    }

    #[test]
    fn test_predicate_of() {
        assert_eq!(predicate_of("distance(hospital)"), "distance");
        assert_eq!(predicate_of("distance(A, B)"), "distance");
        assert_eq!(predicate_of("speed"), "speed");
    }

    #[test]
    fn test_parameterized_comparison_predicate() {
        assert_eq!(
            parameterized_comparison_predicate("distance", '>', 100.0),
            "resin_distance_gt_100"
        );
        assert_eq!(
            parameterized_comparison_predicate("distance", '<', 20.5),
            "resin_distance_lt_20_5"
        );
    }
}
