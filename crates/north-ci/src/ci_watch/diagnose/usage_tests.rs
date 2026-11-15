use super::{HELP_LINES, Request, USAGE, help, request};

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| word.to_string()).collect()
}

#[test]
fn ci_diagnose_usage_errors_and_values_match_the_parser_they_were_captured_from() {
    assert_eq!(request(&argv(&[])), Ok(Request::All));
    assert_eq!(request(&argv(&["-h"])), Ok(Request::Help));
    assert_eq!(request(&argv(&["--help"])), Ok(Request::Help));
    assert_eq!(help(), format!("{USAGE}\n{}\n{}", HELP_LINES[0], HELP_LINES[1]));
    for words in [
        &["x"][..],
        &["--dry-run"],
        &["--dry-run", "owner/repo"],
        &["--dry-run", "owner/repo", "not-a-run"],
        &["--dry-run", "owner/repo", "12", "extra"],
        &["--dry-run=owner/repo"],
        &["--help", "x"],
        &["-x"],
    ] {
        assert_eq!(request(&argv(words)), Err(()), "{words:?}");
    }
    assert_eq!(request(&argv(&["--dry-run", "owner/repo", "12"])), Ok(Request::DryRun { repo: "owner/repo".into(), id: 12 }));
    assert_eq!(request(&argv(&["--dry-run", "--odd", "7"])), Ok(Request::DryRun { repo: "--odd".into(), id: 7 }));
    assert_eq!(request(&argv(&["--"])), Ok(Request::All));
    assert_eq!(request(&argv(&["--dry-run", "--", "owner/repo", "12"])), Ok(Request::DryRun { repo: "owner/repo".into(), id: 12 }));
}
