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

pub fn get_literals(body: &str) -> Vec<String> {
    let body = body.replace("and", "");

    LITERAL_REGEX
        .find_iter(&body)
        .map(|m| m.as_str().to_owned())
        .collect()
}

#[cfg(test)]
mod tests {

    use super::*;

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
}
