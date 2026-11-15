use super::parse;

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

#[test]
fn private_check_usage_errors_and_values_match_the_parser_they_were_captured_from() {
    for (words, error) in [
        (&["--config"][..], "--config needs a value"),
        (&["--check"], "--check needs a value"),
        (&["--ref"], "--ref needs a value"),
        (&["--since"], "--since needs a value"),
        (&["--bogus"], "unknown argument --bogus"),
        (&["--help"], "unknown argument --help"),
        (&["-h"], "unknown argument -h"),
        (&["stray"], "unknown argument stray"),
        (&["--dry-run=yes"], "unknown argument --dry-run=yes"),
        (&["--", "stray"], "unknown argument --"),
        (&["-x"], "unknown argument -x"),
        (&["--ref", "main"], "--ref and --since judge a chosen commit, so they need --dry-run"),
        (&["--since", "main"], "--ref and --since judge a chosen commit, so they need --dry-run"),
    ] {
        assert_eq!(parse(&argv(words)).err().as_deref(), Some(error), "{words:?}");
    }
    let found = parse(&argv(&[])).unwrap();
    assert_eq!(found.config, None);
    assert_eq!(found.check, None);
    assert_eq!((found.overrides.reference, found.overrides.since, found.overrides.dry_run), (None, None, false));
    let found = parse(&argv(&["--dry-run", "--config", "checks.toml", "--check", "mine", "--ref", "head", "--since", "tip"])).unwrap();
    assert_eq!(found.config.as_deref().map(std::path::Path::to_str), Some(Some("checks.toml")));
    assert_eq!(found.check.as_deref(), Some("mine"));
    assert_eq!((found.overrides.reference.as_deref(), found.overrides.since.as_deref(), found.overrides.dry_run), (Some("head"), Some("tip"), true));
    let found = parse(&argv(&["--config", "--odd", "--dry-run", "--dry-run"])).unwrap();
    assert_eq!(found.config.as_deref().map(std::path::Path::to_str), Some(Some("--odd")));
    assert!(found.overrides.dry_run);
}
