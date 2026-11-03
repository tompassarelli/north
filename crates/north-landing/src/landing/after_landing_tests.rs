use super::*;

const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const NEXT: &str = "fedcba9876543210fedcba9876543210fedcba98";

fn failure(gate: &str) -> Failure {
    Failure { gate: gate.to_owned(), output: format!("safe-push: ✗ {gate} refused") }
}

fn created(head: &str, date: &str, failures: &[Failure]) -> String {
    let Filing::Create { title, body } = filing(head, date, None, failures) else {
        panic!("a failure with no open rolling issue creates it");
    };
    assert_eq!(title, "After-landing gate debt");
    body
}

#[test]
fn the_first_failure_creates_the_rolling_issue_with_one_section_for_the_commit() {
    let body = created(HEAD, "2026-11-04", &[failure("code-health"), failure("ci-budget")]);
    assert!(body.starts_with("Gates that run after landing and failed; fix forward in the next landing of the same lane; each row is one landed commit."), "{body}");
    assert!(body.contains("<!-- after-landing-gate: 0123456789ab code-health -->\n<!-- after-landing-gate: 0123456789ab ci-budget -->\n"), "{body}");
    assert!(body.contains("### 0123456789ab (2026-11-04) gates: code-health, ci-budget\n"), "{body}");
    assert!(body.contains("safe-push: ✗ code-health refused"), "{body}");
}

#[test]
fn a_later_commit_is_edited_in_as_a_new_section_above_the_older_one() {
    let first = created(HEAD, "2026-11-03", &[failure("code-health")]);
    let filed = Filed { number: 7, body: first };
    let Filing::Edit { number: 7, body } = filing(NEXT, "2026-11-04", Some(&filed), &[failure("review")]) else {
        panic!("a new commit edits the open issue");
    };
    let newer = body.find("### fedcba987654 (2026-11-04) gates: review").expect("new section");
    let older = body.find("### 0123456789ab (2026-11-03) gates: code-health").expect("older section kept");
    assert!(newer < older, "{body}");
    assert_eq!(body.matches("### ").count(), 2, "{body}");
}

#[test]
fn a_gate_already_marked_for_the_commit_is_not_added_again_but_a_new_gate_is() {
    let filed = Filed { number: 7, body: created(HEAD, "2026-11-04", &[failure("code-health"), failure("ci-budget")]) };
    assert_eq!(filing(HEAD, "2026-11-04", Some(&filed), &[failure("ci-budget")]), Filing::Nothing);
    let Filing::Edit { number: 7, body } = filing(HEAD, "2026-11-04", Some(&filed), &[failure("ci-budget"), failure("review")]) else {
        panic!("a new gate on the same commit edits the issue");
    };
    assert_eq!(body.matches("0123456789ab review -->").count(), 1, "{body}");
    assert_eq!(body.matches("0123456789ab ci-budget -->").count(), 1, "{body}");
    assert!(filing(NEXT, "2026-11-04", Some(&filed), &[failure("ci-budget")]) != Filing::Nothing);
}

#[test]
fn each_gate_keeps_only_its_last_fifteen_output_lines() {
    let output = (1..=40).map(|line| format!("line {line}")).collect::<Vec<_>>().join("\n");
    let body = created(HEAD, "2026-11-04", &[Failure { gate: "g".to_owned(), output }]);
    assert!(body.contains("line 40") && body.contains("line 26") && !body.contains("line 25\n"), "{body}");
}

#[test]
fn the_size_cap_drops_the_oldest_sections_and_the_header_counts_them() {
    let big = |gate: &str| Failure { gate: gate.to_owned(), output: "x".repeat(2000) + "\n" + &"y".repeat(2000) };
    let mut body = created(&format!("{:012}{}", 0, "a".repeat(28)), "2026-11-01", &[big("g")]);
    for index in 1..=30 {
        let head = format!("{index:012}{}", "a".repeat(28));
        let filed = Filed { number: 7, body };
        let Filing::Edit { body: edited, .. } = filing(&head, "2026-11-02", Some(&filed), &[big("g")]) else {
            panic!("a new commit edits the issue");
        };
        assert!(edited.chars().count() <= 60_000, "{}", edited.chars().count());
        body = edited;
    }
    assert!(body.contains("### 000000000030 "), "the newest section stays");
    assert!(!body.contains("### 000000000000 "), "the oldest section is dropped");
    let dropped: usize = body.lines().find_map(|line| line.strip_prefix("Older sections dropped: ")?.trim_end_matches('.').parse().ok()).expect("header names the drop count");
    assert_eq!(dropped + body.matches("### ").count(), 31, "{body}");
}

#[test]
fn ssh_and_https_origins_file_their_issue_in_the_same_repository() {
    for url in ["git@github.com:tompassarelli/north.git", "https://github.com/tompassarelli/north", "ssh://git@github.com/tompassarelli/north.git\n"] {
        assert_eq!(repo_slug(url).as_deref(), Some("tompassarelli/north"), "{url}");
    }
    assert_eq!(repo_slug("/srv/git/north.git"), None);
}

#[test]
fn a_failing_repo_check_files_its_output_on_the_landed_commit_and_a_passing_one_files_nothing() {
    let checks = repo_checks("standalone-parity :: echo 'FAIL pad: first divergent frame 412'; exit 1\nfine :: true\nno separator\n");
    assert_eq!(checks.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["standalone-parity", "fine"]);
    let tree = std::env::temp_dir();
    let failures = repo_check_failures(&tree, &checks);
    assert_eq!(failures.iter().map(|failure| failure.gate.as_str()).collect::<Vec<_>>(), ["standalone-parity"]);
    let body = created(NEXT, "2026-11-04", &failures);
    assert!(body.contains("gates: standalone-parity") && body.contains("first divergent frame 412"), "{body}");
}

#[test]
fn a_linked_path_from_the_main_checkout_is_present_for_a_repo_check() {
    let root = std::env::temp_dir().join(format!("north-after-landing-link-{}", std::process::id()));
    let main = root.join("main");
    let tree = root.join("tree");
    fs::create_dir_all(main.join("ts/node_modules/wisp")).unwrap();
    fs::create_dir_all(tree.join("ts")).unwrap();
    fs::write(main.join("ts/node_modules/wisp/marker"), "x").unwrap();
    link_paths(&main, &tree, "ts/node_modules\n\n").unwrap();
    let checks = repo_checks("needs-link :: test -e ts/node_modules/wisp/marker");
    let failures = repo_check_failures(&tree, &checks);
    let _ = fs::remove_dir_all(&root);
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn the_checkout_on_main_is_the_link_source_even_when_another_worktree_is_listed_first() {
    let listing = "worktree /code/x/worktrees/test-loop\nHEAD aaa\nbranch refs/heads/test-loop\n\nworktree /code/x/main\nHEAD bbb\nbranch refs/heads/main\n\nworktree /code/x/other\nHEAD ccc\ndetached\n";
    assert_eq!(main_worktree(listing), Some(PathBuf::from("/code/x/main")));
    assert_eq!(main_worktree("worktree /code/y\nHEAD aaa\ndetached\n"), Some(PathBuf::from("/code/y")));
}
