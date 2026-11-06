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
}
