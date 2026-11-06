use super::*;

fn landing(sha: &str, files: &[&str], cause: Option<Cause>, fixed_by: Option<&str>) -> Judged {
    Judged { repo: "north".into(), at: 1, cause, sha: sha.into(), files: files.iter().map(|file| (*file).to_owned()).collect(), fixed_by: fixed_by.map(str::to_owned) }
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
    let text = render(&landings(judged.iter()), "in the last 7 days");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "north scoreboard rework: 2 of 3 landings that reached main in the last 7 days were reworked within 48 h (67%), by class of change");
    assert_eq!(lines[1], "- control-plane code: 2 of 2 (100%): reverted 1, fixed forward 1, red-main culprit 0");
    assert!(lines.contains(&"- docs: 0 of 1 (0%): reverted 0, fixed forward 0, red-main culprit 0"));
}
