use super::*;

fn coverage(entries: &[(&str, &[&str])]) -> BTreeMap<String, BTreeSet<String>> {
    entries.iter().map(|(command, paths)| ((*command).to_owned(), paths.iter().map(|path| (*path).to_owned()).collect())).collect()
}

#[test]
fn a_waiver_needs_a_single_word_name_and_a_reason_after_because() {
    let messages = "fix (north#360)\n\nRemoves-check: lint.sh because clippy covers it\nRemoves-check: no reason\nRemoves-check: two words because x\nRemoves-check: empty because \n";
    assert_eq!(waivers(messages), vec![Waiver { name: "lint.sh", reason: "clippy covers it" }]);
}

#[test]
fn a_waiver_names_a_check_by_its_command_a_token_or_a_tokens_file_name() {
    let command = "CARGO_TARGET_DIR=target bash harness/scripts/check-owners.sh --strict";
    assert!(waives("check-owners.sh", command));
    assert!(waives("harness/scripts/check-owners.sh", command));
    assert!(waives(command, command));
    assert!(!waives("check-owners", command));
    assert!(!waives("--strict", command));
    assert!(!waives("CARGO_TARGET_DIR=target", command));
}

#[test]
fn a_narrowed_check_leaves_the_paths_it_dropped_uncovered() {
    let before = coverage(&[("lint", &["src/a.rs", "src/b.rs"])]);
    let after = coverage(&[("lint", &["src/a.rs"])]);
    assert_eq!(uncovered(&before, &after), vec![Uncovered { command: "lint".into(), name: "lint".into(), paths: vec!["src/b.rs".into()] }]);
}

#[test]
fn a_new_check_covering_the_dropped_paths_replaces_the_removed_one() {
    let before = coverage(&[("old-lint", &["src/a.rs", "src/b.rs"])]);
    assert!(uncovered(&before, &coverage(&[("new-lint", &["src/a.rs", "src/b.rs"])])).is_empty());
    let partial = uncovered(&before, &coverage(&[("new-lint", &["src/a.rs"])]));
    assert_eq!(partial[0].paths, vec!["src/b.rs".to_owned()]);
}

#[test]
fn a_check_that_already_existed_does_not_replace_a_removed_one() {
    let before = coverage(&[("lint", &["src/a.rs"]), ("test", &["tests/t.rs"])]);
    let after = coverage(&[("test", &["tests/t.rs", "src/a.rs"])]);
    assert_eq!(uncovered(&before, &after)[0].paths, vec!["src/a.rs".to_owned()]);
}

#[test]
fn the_suggested_name_is_the_first_token_no_other_check_shares() {
    let before = coverage(&[("bash scripts/one.sh", &["a"]), ("bash scripts/two.sh", &["b"])]);
    let gaps = uncovered(&before, &BTreeMap::new());
    let names: Vec<&str> = gaps.iter().map(|gap| gap.name.as_str()).collect();
    assert_eq!(names, vec!["one.sh", "two.sh"]);
}
