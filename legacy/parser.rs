use crate::language::concepts::{Clause, Source, Target};

// Individual language elements and named groups
const ATOM_PATTERN: String = r"(?<atom>\w+)".to_string();
const LITERAL_PATTERN: String = r"(?<literal>(?:not\s+)?\w+)".to_string();
const PROBABILITY_PATTERN: String = r"Probability\((?<probability>[01][.]\d+)\)".to_string();
const BODY_PATTERN: String = r"(?<body>.+)".to_string();
const TOPIC_PATTERN: String = r#""(?<topic>(?:\/\w+)+)""#.to_string();
const DTYPE_PATTERN: String = r"(?<dtype>Probability|Density|Number)".to_string();

// Regular expressions for complete Resin statements
const LITERAL_REGEX: Regex = Regex::new(&format!(r"(?:\s+and\s+)?{}", LITERAL_PATTERN)).unwrap();
const CLAUSE_REGEX: Regex = Regex::new(&format!(
    r"{}(\s+<-\s+{})?(\s+if\s+{})?\.",
    ATOM_PATTERN, probability, body
))
.unwrap();
const SOURCE_REGEX: Regex = Regex::new(&format!(
    r#"{}\s+<-\s+source\({},\s+{}\)\."#,
    ATOM_PATTERN, topic, dtype
))
.unwrap();
const TARGET_REGEX: Regex = Regex::new(&format!(r#"{}\s+->\s+target\({}\)\."#, ATOM_PATTERN, topic)).unwrap();


pub fn parse(model: String) -> Vec<ReactiveCircuit> {
    panic::set_hook(Box::new(|_info| {}));

    // Parse Resin source line by line into appropriate data structures
    let mut program = "".to_string();
    let mut targets = vec![];
    for line in model.lines() {
        let source: Source = line.parse();
        
        let target: Target = line.parse();
        let clause: Clause = line.parse();
    }

    // Pass data to Clingo and obtain stable models
    for target in targets {
        let target_program = program.clone() + &target;
        println!("\n{}\n", &target_program);

        let mut ctl =
            control(vec!["--models=0".to_string()]).expect("Failed creating clingo_control.");
        ctl.add("base", &[], &target_program)
            .expect("Failed to add a logic program.");

        // ground the base part
        let part = Part::new("base", vec![]).unwrap();
        let parts = vec![part];
        ctl.ground(&parts)
            .expect("Failed to ground a logic program.");

        // solve
        solve(ctl);
    }

    let _ = panic::take_hook();

    // Create a Reactive Circuits for each target signal
    return Vec::new();
}
