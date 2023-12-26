use lazy_static::lazy_static;
use regex::Regex;

// Individual language elements and named groups
const ATOM_PATTERN: &str = r"(?<atom>\w+(\([\w\s,]+\))?)";
const LITERAL_PATTERN: &str = r"(?<literal>(not\s+)?\w+(\([\w\s,]+\))?)";
const PROBABILITY_PATTERN: &str = r"P\((?<probability>[01][.]\d+)\)";
const BODY_PATTERN: &str = r"(?<body>.+)";
const TOPIC_PATTERN: &str = r#""(?<topic>(?:\/\w+)+)""#;
const DTYPE_PATTERN: &str = r"(?<dtype>Probability|Density|Number)";
const VARIABLE_LIST_PATTERN: &str = r"((?:\()(?:(?:,\s+)?\w+)+(?:\)))";
const VARIABLE_PATTERN: &str = r"((?:(,\s+)?)(?<variable>[A-Z]))";

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
}


pub fn get_literals(body: &str) -> Vec<String> {
    let body = body.replace("and", "");
    
    LITERAL_REGEX.find_iter(&body).map(|m| m.as_str().to_owned()).collect()
}


#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_literal() {
        let input = "test";
        let Some(captures) = LITERAL_REGEX.captures(input) else { panic!() };
        assert_eq!(&captures["literal"], input);
        
        let input = "test(a)";
        let Some(captures) = LITERAL_REGEX.captures(input) else { panic!() };
        assert_eq!(&captures["literal"], input);

        let input = "test(a, b)";
        let Some(captures) = LITERAL_REGEX.captures(input) else { panic!() };
        assert_eq!(&captures["literal"], input);

        let input = "not test(a_1, b, c)";
        let Some(captures) = LITERAL_REGEX.captures(input) else { panic!() };
        assert_eq!(&captures["literal"], input);
    }

    #[test]
    fn test_body() {
        let input = "a if test.";
        let Some(captures) = CLAUSE_REGEX.captures(input) else { panic!() };
        assert_eq!(&captures["body"], "test");

        let input = "a if test and other.";
        let Some(captures) = CLAUSE_REGEX.captures(input) else { panic!() };
        assert_eq!(&captures["body"], "test and other");

        let input = "a(X, Y) if test and other.";
        let Some(captures) = CLAUSE_REGEX.captures(input) else { panic!() };
        assert_eq!(&captures["body"], "test and other");

        let input = "a_b(X, some_thing) <- P(0.4) if test(X) and other(some_thing, C).";
        let Some(captures) = CLAUSE_REGEX.captures(input) else { panic!() };
        assert_eq!(&captures["atom"], "a_b(X, some_thing)");
        assert_eq!(&captures["body"], "test(X) and other(some_thing, C)");
        assert_eq!(get_literals(&captures["body"]), vec!["test(X)", "other(some_thing, C)"]);
    }

}