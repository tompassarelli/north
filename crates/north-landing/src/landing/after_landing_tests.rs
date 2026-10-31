use super::*;

fn failure(gate: &str) -> Failure {
    Failure { gate: gate.to_owned(), output: format!("safe-push: ✗ {gate} refused") }
}

#[test]
fn a_landed_commit_gets_one_issue_and_each_failed_gate_is_filed_on_it_once() {
    let head = "0123456789abcdef0123456789abcdef01234567";
    let Filing::Create { title, body } = filing("o/r", head, None, &[failure("code-health"), failure("test-kind")]) else {
        panic!("a first failure files an issue");
    };
    assert_eq!(title, "After-landing gates failed on 0123456789ab");
    assert!(body.contains("failed on it: code-health, test-kind."), "{body}");
    let filed = Filed { number: 7, text: body };
    assert_eq!(filing("o/r", head, Some(&filed), &[failure("test-kind")]), Filing::Nothing);
    let Filing::Comment { number: 7, body } = filing("o/r", head, Some(&filed), &[failure("test-kind"), failure("review")]) else {
        panic!("a new gate on the same commit comments on its issue");
    };
    assert!(body.contains("failed on it: review.") && !body.contains("### test-kind"), "{body}");
}

#[test]
fn ssh_and_https_origins_file_their_issue_in_the_same_repository() {
    for url in ["git@github.com:tompassarelli/north.git", "https://github.com/tompassarelli/north", "ssh://git@github.com/tompassarelli/north.git\n"] {
        assert_eq!(repo_slug(url).as_deref(), Some("tompassarelli/north"), "{url}");
    }
    assert_eq!(repo_slug("/srv/git/north.git"), None);
}
