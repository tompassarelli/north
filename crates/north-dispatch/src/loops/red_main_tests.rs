use super::unwatched;

fn run(sha: &str, minute: u32, success: bool) -> String {
    let conclusion = if success { "success" } else { "failure" };
    format!(r#"{{"path":".github/workflows/cargo.yml","head_branch":"main","event":"push","status":"completed","conclusion":"{conclusion}","head_sha":"{sha}","created_at":"2026-11-06T06:{minute:02}:00Z","updated_at":"2026-11-06T06:{minute:02}:30Z"}}"#)
}

fn history(runs: &[String]) -> String {
    format!(r#"{{"workflow_runs":[{}]}}"#, runs.join(","))
}

#[test]
fn a_main_red_for_over_10_minutes_with_no_revert_and_no_naming_is_unwatched() {
    let began = 1_700_000_000;
    let now = 1_794_000_000;
    let red = run("aaaaaaaaaaaaaaaa", 10, false);
    let long = history(&[run("bbbbbbbbbbbbbbbb", 40, true), red.clone(), run("cccccccccccccccc", 5, true)]);
    assert_eq!(unwatched(&long, began, now, &|_| false, &|_| false).unwrap().len(), 1);
    assert!(unwatched(&long, began, now, &|sha| sha.starts_with('a'), &|_| false).unwrap().is_empty(), "a fleet message naming it covers it");
    assert!(unwatched(&long, began, now, &|_| false, &|sha| sha.starts_with('a')).unwrap().is_empty(), "a sheriff revert covers it");
    let quick = history(&[run("bbbbbbbbbbbbbbbb", 15, true), red]);
    assert!(unwatched(&quick, began, now, &|_| false, &|_| false).unwrap().is_empty(), "red for under 10 minutes");
    assert!(unwatched(&long, now, now, &|_| false, &|_| false).unwrap().is_empty(), "red from before the watch began recording names");
}
