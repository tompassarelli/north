use super::functions::{SourceText, measure};

#[test]
fn boolean_runs_count_once_while_mixed_operators_and_separate_expressions_stay_distinct() {
    let mut cases: Vec<(String, usize)> = Vec::new();
    for op in ["&&", "||", "??"] {
        for n in 0..7 {
            let operands: Vec<String> = (0..n + 2).map(|i| format!("v{i}")).collect();
            cases.push((format!("export function run() {{ return {}; }}", operands.join(&format!(" {op} "))), 2));
        }
    }
    for (text, complexity) in [
        ("function run() { return a && (b && c); }", 2),
        ("function run() { return a && b || c && d; }", 4),
        ("function run() { const x = a ?? b; return c ?? d; }", 3),
        ("function run() { return a && (b || c) && d; }", 3),
    ] {
        cases.push((text.to_owned(), complexity));
    }
    let files: Vec<SourceText> = cases.iter().enumerate().map(|(i, (text, _))| SourceText { path: format!("{i}.ts"), text: text.as_str().into() }).collect();
    let measured = measure(&[("h", &files)]).unwrap();
    for (i, (text, complexity)) in cases.iter().enumerate() {
        let found: Vec<usize> = measured[&format!("h/{i}.ts")].iter().map(|f| f.complexity).collect();
        assert_eq!(found, [*complexity], "{text}");
    }
}
