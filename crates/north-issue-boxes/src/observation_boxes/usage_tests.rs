use super::options;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

const USAGE: &str = "north task check [--observations] [--dry-run] (--body-file FILE | REPO#N | --repo OWNER/REPO)";

#[test]
fn observation_check_usage_errors_and_values_match_the_parser_they_were_captured_from() {
    for words in [
        &[][..],
        &["--help"],
        &["stray"],
        &["--repo"],
        &["--body-file"],
        &["--bogus", "a#1"],
        &["--repo=a/b"],
        &["--dry-run=yes", "a#1"],
        &["-x", "a#1"],
        &["--", "a#1"],
        &["--dry-run"],
        &["a#1", "--repo", "a/b"],
        &["--body-file", "f", "a#1"],
        &["--body-file", "f", "--repo", "a/b"],
    ] {
        let error = options(&argv(words)).err().unwrap_or_else(|| panic!("{words:?} parsed"));
        assert_eq!(error.to_string(), USAGE, "{words:?}");
        assert_eq!(error.exit_code(), 2, "{words:?}");
    }
    let found = options(&argv(&["--observations", "--dry-run", "a#1", "b#2"])).unwrap();
    assert_eq!((found.dry_run, found.issue.as_deref(), found.repo, found.body_file), (true, Some("b#2"), None, None));
    let found = options(&argv(&["--repo", "--odd", "--repo", "a/b"])).unwrap();
    assert_eq!((found.dry_run, found.issue, found.repo.as_deref(), found.body_file), (false, None, Some("a/b"), None));
    let found = options(&argv(&["--body-file", "--dash"])).unwrap();
    assert_eq!(found.body_file.as_deref(), Some("--dash"));
}
