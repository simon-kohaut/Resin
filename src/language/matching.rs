use lazy_static::lazy_static;
use regex::Regex;

// Individual language elements and named groups
const ATOM_PATTERN: &str = r"(?<atom>[\w\(\)]+)";
const LITERAL_PATTERN: &str = r"(?<literal>(?:not\s+)?[\w\(\)]+)";
const PROBABILITY_PATTERN: &str = r"P\((?<probability>[01][.]\d+)\)";
const BODY_PATTERN: &str = r"(?<body>.+)";
const TOPIC_PATTERN: &str = r#""(?<topic>(?:\/\w+)+)""#;
const DTYPE_PATTERN: &str = r"(?<dtype>Probability|Density|Number)";
const VARIABLE_LIST_PATTERN: &str = r"((?:\()(?:(?:,\s+)?\w+)+(?:\)))";
const VARIABLE_PATTERN: &str = r"((?:(,\s+)?)(?<variable>[A-Z]))";

// Regular expressions for complete Resin statements
lazy_static! {
    pub static ref LITERAL_REGEX: Regex =
        Regex::new(&format!(r"(?:\s+and\s+)?{}", LITERAL_PATTERN)).unwrap();
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
