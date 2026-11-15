use super::options;
use std::path::PathBuf;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

const USAGE: &str = "north task rewrite-cargo [--dry-run] [--repo OWNER/REPO] [--tree DIR]";

#[test]
fn rewrite_cargo_usage_errors_and_values_match_the_parser_they_were_captured_from() {
    for words in [
        &["--help"][..],
        &["stray"],
        &["--repo"],
        &["--tree"],
        &["--bogus"],
        &["--repo=a/b"],
        &["--dry-run=yes"],
        &["-x"],
        &["-"],
        &["--"],
    ] {
        let error = options(&argv(words)).err().unwrap_or_else(|| panic!("{words:?} parsed"));
        assert_eq!(error.to_string(), USAGE, "{words:?}");
        assert_eq!(error.exit_code(), 2, "{words:?}");
    }
    let found = options(&argv(&[])).unwrap();
    assert_eq!((found.dry_run, found.repo.as_str(), found.tree), (false, "tompassarelli/north", PathBuf::from(".")));
    let found = options(&argv(&["--dry-run", "--repo", "a/b", "--tree", "--odd", "--repo", "c/d"])).unwrap();
    assert_eq!((found.dry_run, found.repo.as_str(), found.tree), (true, "c/d", PathBuf::from("--odd")));
}
