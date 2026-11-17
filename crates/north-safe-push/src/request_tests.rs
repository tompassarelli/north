use super::*;

#[test]
fn safe_push_refuses_destination_names_git_would_misread() {
    for invalid in ["HEAD", "refs/heads/main", "origin/main", "../main", "bad name", "bad..name", "a@{1}", "a~1", "a^", "a:b", "a?", "a*", "a[b", "a\\b"] {
        let arguments = ["--to".to_owned(), invalid.to_owned()];
        match super::arguments(&arguments) {
            Err(refusal) => {
                assert_eq!(refusal.exit_code(), 2, "{invalid}");
                assert_eq!(refusal.stderr(), format!("safe-push: invalid destination branch '{invalid}'\n{USAGE}"), "{invalid}");
            }
            Ok(_) => panic!("{invalid} was accepted"),
        }
    }
    assert_eq!(super::arguments(&["--check-only".to_owned()]), Ok(Plan::Push(Arguments { check_only: true, ..Arguments::default() })));
    assert_eq!(super::arguments(&["--check-only".to_owned(), "--to".to_owned(), "main".to_owned()]), Ok(Plan::Push(Arguments { check_only: true, to: Some("main".to_owned()), ..Arguments::default() })));
    assert!(super::destination(&["--to".to_owned(), "main".to_owned(), "--check-only".to_owned()]).is_none());
    for invalid in [
        &["--check-only", "--dry-run"][..],
        &["--check-only", "--wait"][..],
        &["--check-only", "--keep-lane", "--to", "main"][..],
        &["--check-only", "--direct"][..],
        &["--check-only", "--tag", "v1"][..],
        &["--check-only", "--delete-tag", "v1"][..],
        &["--require-check-only"][..],
    ] {
        let words: Vec<String> = invalid.iter().map(|word| (*word).to_owned()).collect();
        assert!(super::arguments(&words).is_err(), "{invalid:?} was accepted");
    }
}
