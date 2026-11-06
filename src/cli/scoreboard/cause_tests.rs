use super::*;
use crate::threads::calibration::rework::{Commit, Landing, Trigger, Verdict, judge_history};

fn landing(sha: &str, files: &[&str], cause: Option<Cause>, fixed_by: Option<&str>) -> Judged {
    let rule = cause.map(|cause| if cause == Cause::Reverted { Rule::Revert } else { Rule::NamedIssue });
    let trigger = fixed_by.map(|subject| Trigger { sha: "f".repeat(40), subject: subject.to_owned() });
    Judged { repo: "north".into(), at: 1, cause, sha: sha.into(), files: files.iter().map(|file| (*file).to_owned()).collect(), rule, trigger }
}

#[test]
fn a_landing_takes_the_class_most_of_its_files_fall_in() {
    let files = |paths: &[&str]| paths.iter().map(|path| (*path).to_owned()).collect::<Vec<_>>();
    assert_eq!(Class::of_landing(&files(&["src/cli/mod.rs", "src/cli/a_tests.rs", "src/cli/b_tests.rs"])), Class::Tests);
    assert_eq!(Class::of_landing(&files(&["harness/loops.toml", "docs/a.md"])), Class::Tables);
    assert_eq!(Class::of_landing(&files(&["crates/north-guard/src/x.rs"])), Class::Gates);
    assert_eq!(Class::of_landing(&[]), Class::Code);
}

#[test]
fn the_largest_reworking_class_comes_first_and_one_landing_counts_once() {
    let judged = [
        landing("a", &["docs/a.md"], None, None),
        landing("b", &["src/x.rs"], Some(Cause::FixedForward), Some("Fix x")),
        landing("c", &["src/y.rs"], Some(Cause::Reverted), Some("Revert y")),
        landing("c", &["src/y.rs"], None, None),
    ];
    let text = render(&landings(judged.iter()), "7d", 0);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "north: 2 of 3 judged landings reworked within 48 h (67%)");
    assert_eq!(lines[2], "  by rule: revert 1, named commit 0, named issue 1, Fix subject with file overlap 0, red-main culprit 0");
    assert_eq!(lines[4], "  - control-plane code: 2 of 2 (100%): reverted 1, fixed forward 1, red-main culprit 0");
    assert_eq!(lines[5], "      b reworked by fffffffff Fix x [named issue]");
    assert!(lines.contains(&"  - docs: 0 of 1 (0%): reverted 0, fixed forward 0, red-main culprit 0"));

    let commit = |sha: &str, at: i64, subject: &str, body: &str| Commit { sha: sha.repeat(20), at, subject: subject.into(), body: body.into(), files: vec!["src/x.rs".into()] };
    let history = [commit("a1", 1_000, "Add x", ""), commit("b2", 1_100, "Repair x", "Follows: north#7"), commit("c3", 1_200, "Revert \"Add x\"", "")];
    let asked = Landing { agent: "w".into(), item: Some("north#7".into()), sha: "a1".repeat(20) };
    let verdict = &judge_history(&history, &[&asked])[0].1;
    assert_eq!((verdict.rule, verdict.trigger.as_ref().map(|trigger| trigger.subject.as_str())), (Some(Rule::Revert), Some("Revert \"Add x\"")));
    let stored: Verdict = serde_json::from_str(r#"{"at":5,"cause":"FixedForward","closed":true}"#).unwrap();
    assert!(!stored.recorded());
}
