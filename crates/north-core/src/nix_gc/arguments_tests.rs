use super::parse;

fn accepted(words: &[&str]) -> Option<(bool, Option<String>)> {
    let words: Vec<String> = words.iter().map(|word| word.to_string()).collect();
    parse(&words).ok().map(|options| (options.dry_run, options.max_freed))
}

#[test]
fn flags_are_accepted_and_refused_as_the_parser_they_were_captured_from_did() {
    let size = |text: &str| Some(text.to_owned());
    assert_eq!(accepted(&[]), Some((false, None)));
    assert_eq!(accepted(&["--dry-run"]), Some((true, None)));
    assert_eq!(accepted(&["--max-freed", "5G"]), Some((false, size("5G"))));
    assert_eq!(accepted(&["--max-freed=5G", "--dry-run"]), Some((true, size("5G"))));
    assert_eq!(accepted(&["--max-freed", "1", "--max-freed=2"]), Some((false, size("2"))));
    assert_eq!(accepted(&["--max-freed", "-1"]), Some((false, size("-1"))));
    assert_eq!(accepted(&["--max-freed="]), Some((false, size(""))));
    for refused in [&["--max-freed"][..], &["--dry-run=1"], &["--bogus"], &["stray"], &["-x"], &["-"], &["--"], &["--help"]] {
        assert_eq!(accepted(refused), None, "{refused:?}");
    }
}
