fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

#[test]
fn usage_errors_match_the_parsers_they_were_captured_from() {
    let spend: &[&[&str]] = &[&["--bogus"], &["stray"], &["--json", "--json"], &["--json=1"], &["--"], &["-x"], &["--json", "extra"]];
    for words in spend {
        assert_eq!(super::spend::run(&argv(words)), 2, "spend {words:?}");
    }
    let adopt: &[&[&str]] = &[&["--bogus"], &["stray"], &["--dry-run", "--dry-run"], &["--dry-run=1"], &["--"], &["-x"]];
    for words in adopt {
        assert_eq!(super::land_adopt::run(&argv(words)), 64, "land adopt {words:?}");
    }
    let check: &[&[&str]] = &[&["--tree"], &["--live"], &["stray"], &["--tree=DIR"], &["--tree", "a", "--live"], &["--tree", "a", "--bogus", "b"], &["--"], &["--tree", "a", "b"]];
    for words in check {
        assert_eq!(super::call_check::run(&argv(words), |_| true), 64, "check calls {words:?}");
    }
}
