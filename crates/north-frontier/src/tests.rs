use crate::report::render;
use crate::rows::{Arm, Run, parse};
use crate::stats::{Pairs, mcnemar, wilson};

fn run(task: &str, arm: Arm, first: bool) -> Run {
    Run { task: task.into(), arm, correct_first_pass: first, correct: first, cost_usd: 2.0, wall_seconds: 600.0, interventions: 0, contaminated: false }
}

#[test]
fn wilson_matches_the_published_interval_for_eight_of_ten() {
    let interval = wilson(8, 10).unwrap();
    assert!((interval.low - 0.4902).abs() < 1e-3 && (interval.high - 0.9433).abs() < 1e-3);
}

#[test]
fn mcnemar_is_exact_on_the_discordant_split() {
    assert!((mcnemar(Pairs { both: 5, only_first: 6, only_second: 1, neither: 0 }) - 0.125).abs() < 1e-12);
    assert_eq!(mcnemar(Pairs { both: 3, only_first: 0, only_second: 0, neither: 2 }), 1.0);
}

#[test]
fn a_contaminated_pair_leaves_the_headline() {
    let mut tainted = run("a", Arm::Plain, true);
    tainted.contaminated = true;
    let text = render(&[run("a", Arm::North, true), tainted, run("b", Arm::North, true), run("b", Arm::Plain, false)]);
    assert!(text.contains("on 1 boxes both arms ran (1 left out"), "{text}");
    assert!(text.contains("north minus plain: +100 points"), "{text}");
}

#[test]
fn rows_parse_from_json_lines_and_name_the_bad_line() {
    let good = r#"{"task":"north#1","arm":"north","correct_first_pass":true,"correct":true,"cost_usd":1.5,"wall_seconds":90.0,"interventions":1}"#;
    assert_eq!(parse(&format!("{good}\n\n{good}\n")).unwrap().len(), 2);
    assert!(parse(&format!("{good}\nnot json\n")).unwrap_err().contains(":2:"));
}
