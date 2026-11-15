use std::path::Path;

use north_foundation::test_inputs::git::git_in;
use north_landing::land::scans::outgoing::private_file;

use super::{Event, Report};

fn commit(dir: &Path, file: &str, text: &str) -> String {
    let target = dir.join(file);
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::write(&target, text).unwrap();
    git_in(dir, &["add", file]);
    git_in(dir, &["commit", "-qm", file]);
    git_in(dir, &["rev-parse", "HEAD"])
}

#[test]
fn the_land_check_outcome_file_reads_as_the_runner_writes_it_and_a_private_file_copy_is_refused_until_the_operator_approves_it_and_an_unreadable_operator_checkout_refuses() {
    let outcome = "event\tland check\tfailed\tcargo test; log: /tmp/log\nskip\tskipped: bun install failed\nred-main\tabc123\nran\nunknown\tline\n";
    assert_eq!(
        Report::read(outcome),
        Report {
            events: vec![Event { stage: "land check".to_owned(), status: "failed".to_owned(), detail: "cargo test; log: /tmp/log".to_owned() }],
            skip: Some("skipped: bun install failed".to_owned()),
            red_main: Some("abc123".to_owned()),
            ran: true,
        }
    );
    assert_eq!(Report::read(""), Report::default());

    let scratch = tempfile::tempdir().unwrap();
    let (operator, public, cache) = (scratch.path().join("operator"), scratch.path().join("public"), scratch.path().join("cache"));
    for repository in [&operator, &public] {
        std::fs::create_dir_all(repository).unwrap();
        git_in(repository, &["init", "-q"]);
    }
    commit(&operator, "agents/decisions/plan.md", "the private plan\n");
    git_in(&operator, &["rm", "-q", "agents/decisions/plan.md"]);
    git_in(&operator, &["commit", "-qm", "remove"]);
    let base = commit(&public, "README.md", "public\n");
    let head = commit(&public, "docs/copied.md", "the private plan\n");
    let refused = private_file::outgoing(&public, &operator, &cache, (&base, &head)).unwrap();
    assert_eq!(refused.into_iter().collect::<Vec<_>>(), ["private-file: docs/copied.md"], "a file from the operator's history is refused by path, even after the operator deleted it");
    let blob = git_in(&public, &["rev-parse", &format!("{head}:docs/copied.md")]);
    commit(&operator, "agents/approved-moves.toml", &format!("moves = [\"docs/copied.md {blob}\"]\n"));
    assert!(private_file::outgoing(&public, &operator, &cache, (&base, &head)).unwrap().is_empty(), "the operator's approved-moves list unlocks the path");
    assert!(private_file::hits(&public, &operator, &cache, &format!("{base}..{head}"), "").unwrap().is_empty(), "an approved path is not a hit in the sweep either");
    let head = commit(&public, "notes/ok.md", "unrelated\n");
    assert!(private_file::outgoing(&public, &operator, &cache, (&base, &head)).unwrap().is_empty());
    let not_a_repository = scratch.path().join("not-a-repository");
    std::fs::create_dir_all(&not_a_repository).unwrap();
    let refusal = private_file::outgoing(&public, &not_a_repository, &cache, (&base, &head)).unwrap_err();
    assert!(refusal.contains(&not_a_repository.display().to_string()) && refusal.contains("unreadable"), "a configured operator checkout that is not a repository refuses, naming it: {refusal}");
}
