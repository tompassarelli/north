use super::{Child, Failure, children};

#[test]
fn an_advisory_after_landing_check_files_what_it_prints_on_stdout_even_when_it_exits_zero_and_not_its_stderr_notes() {
    let directory = tempfile::tempdir().unwrap();
    let checks = [("code-comment habit".to_owned(), "echo 'code-comment lib.rs:3 narrates the change'".to_owned())];
    let failures = super::repo_advisory_failures(directory.path(), &checks, "base", "head");
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].gate, "code-comment habit");
    assert!(failures[0].output.contains("code-comment lib.rs:3"), "{:?}", failures[0].output);
    let quiet = [("quiet".to_owned(), "echo 'head-north: running the north this host built from tree 0a57932f1231' >&2".to_owned())];
    assert!(super::repo_advisory_failures(directory.path(), &quiet, "base", "head").is_empty());
}

#[test]
fn a_failed_gate_is_filed_as_a_child_of_its_tracker() {
    let failures = [Failure { gate: "code-health".into(), output: "hotspot".into(), check: "north land scan code-health base..head".into() }];
    let filed: Vec<Child> = children(722, "bbbbbbbbbbbbbbbb", "2026-11-06", &[], &failures);
    let [child] = filed.as_slice() else { panic!("one failed gate files one child: {filed:?}") };
    assert_eq!(child.parent, 722);
    assert!(child.body.contains("Done when: `north land scan code-health base..head` exits 0."), "a child names its runnable check: {}", child.body);
    assert!(["Category: ", "Spec: ", "Scope: ", "Class: "].iter().all(|field| child.body.lines().any(|line| line.starts_with(field))), "dispatch needs the four header lines: {}", child.body);
    assert!(!child.body.lines().any(|line| line.trim_start().starts_with("- [")), "a child holds no checkbox line: {}", child.body);
    let open = [child.title.clone()];
    assert!(children(722, "bbbbbbbbbbbbbbbb", "2026-11-06", &open, &failures).is_empty(), "an open child already files this gate");
    let older_commit = ["After-landing gate debt: code-health failed on aaaaaaaaaaaa".to_owned()];
    assert!(children(722, "bbbbbbbbbbbbbbbb", "2026-11-06", &older_commit, &failures).is_empty(), "an open child under an older commit already files this gate");
}
