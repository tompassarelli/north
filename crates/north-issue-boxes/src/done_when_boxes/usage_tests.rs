use super::{USAGE, options};
use std::path::PathBuf;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

#[test]
fn done_when_check_usage_errors_and_values_match_the_parser_they_were_captured_from() {
    for words in [
        &[][..],
        &["--help"],
        &["stray"],
        &["--repo"],
        &["--body-file"],
        &["--main"],
        &["--box"],
        &["--bogus", "a#1"],
        &["--repo=a/b"],
        &["--confined=yes", "a#1"],
        &["-x", "a#1"],
        &["--", "a#1"],
        &["--dry-run"],
        &["a#1", "--repo", "a/b"],
        &["--body-file", "f", "a#1"],
    ] {
        let error = options(&argv(words)).err().unwrap_or_else(|| panic!("{words:?} parsed"));
        assert_eq!(error.to_string(), USAGE, "{words:?}");
        assert_eq!(error.exit_code(), 2, "{words:?}");
    }
    let found = options(&argv(&["--done-when", "--dry-run", "--confined", "--main", "--odd", "--box", "x y", "a#1", "b#2"])).unwrap();
    assert_eq!(
        (found.dry_run, found.confined, found.main, found.only.as_deref(), found.issue.as_deref(), found.repo, found.body_file),
        (true, true, Some(PathBuf::from("--odd")), Some("x y"), Some("b#2"), None, None)
    );
    let found = options(&argv(&["--repo", "a/b", "--repo", "c/d"])).unwrap();
    assert_eq!(found.repo.as_deref(), Some("c/d"));
}
