use std::fs;
use std::path::PathBuf;

use super::{Fixture, git, north_enqueue_with, stub_gh};

fn github_push_url(fixture: &Fixture, archived: &str) -> PathBuf {
    git(&fixture.repo, &["remote", "set-url", "--push", "origin", "https://github.com/o/wake.git"]);
    fixture.commit(&fixture.repo, "notes.txt", "Add notes");
    stub_gh(fixture, archived)
}

#[test]
fn enqueue_refuses_a_repository_github_reports_archived_and_writes_no_ticket() {
    let fixture = Fixture::new();
    let path = github_push_url(&fixture, "echo true");
    let (output, _) = north_enqueue_with(&fixture, &[("PATH", &path)]);
    let text = Fixture::output_of(&output);
    assert!(!output.status.success(), "{text}");
    assert!(text.contains("GitHub reports o/wake as archived") && text.contains("NOT queued"), "{text}");
    assert_eq!(fs::read_to_string(fixture.root.join("gh-calls")).unwrap(), "api repos/o/wake --jq .archived\n");
    assert!(!fixture.state.exists(), "a refused lane writes no ticket to the queue or the outbox");
}

#[test]
fn enqueue_queues_when_github_cannot_say_whether_the_repository_is_archived() {
    let fixture = Fixture::new();
    let path = github_push_url(&fixture, "echo 'error connecting to api.github.com' >&2; exit 1");
    let (output, _) = north_enqueue_with(&fixture, &[("PATH", &path)]);
    assert!(output.status.success(), "{}", Fixture::output_of(&output));
    assert_eq!(fixture.pending().len(), 1);
}
