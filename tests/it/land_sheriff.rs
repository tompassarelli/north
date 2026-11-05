use std::fs;

use super::{Fixture, git};

fn landed_pair(script: &str) -> (Fixture, String) {
    let mut fixture = Fixture::new();
    fs::create_dir_all(fixture.repo.join("tests")).unwrap();
    fs::write(fixture.repo.join("tests/app.test.sh"), script).unwrap();
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "suite"]);
    git(&fixture.repo, &["push", "-q", "origin", "main"]);
    fixture.base = fixture.main();
    let older = fixture.lane("older", "app.txt", "broken\n");
    let newer = fixture.lane("newer", "b.txt", "b");
    fixture.enqueue(&older, None);
    fixture.enqueue(&newer, None);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from older", "change from newer"]);
    let parent = fixture.base.clone();
    (fixture, parent)
}

fn request_revert(fixture: &Fixture, head: &str, parent: &str) {
    let message = format!("Main red at {head}\n\nland check: tests/app.test.sh exited 1\n\nGreen-parent: {parent}\nMain-red-run: https://example/runs/9");
    let request = git(&fixture.repo, &["commit-tree", &format!("{head}^{{tree}}"), "-p", head, "-m", &message]);
    git(&fixture.repo, &["push", "-q", "origin", &format!("{request}:refs/heads/claude/land-revert-{head}")]);
}

fn sheriff_record(fixture: &Fixture, head: &str) -> String {
    fs::read_to_string(fixture.queue().join("sheriff").join(head)).unwrap_or_default()
}

const SUITE: &str = "#!/usr/bin/env bash\n[ \"$(cat app.txt)\" != broken ]\n";

#[test]
fn sheriff_queues_the_culprit_revert_first() {
    let (mut fixture, parent) = landed_pair(SUITE);
    let head = fixture.main();
    request_revert(&fixture, &head, &parent);
    let later = fixture.lane("later", "c.txt", "c");
    fixture.enqueue(&later, None);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from older", "change from newer", "Revert change from older", "change from later"]);
    assert_eq!(git(&fixture.origin, &["show", "main:app.txt"]), "alpha\nbeta\ngamma");
    let body = git(&fixture.origin, &["log", "-1", "--format=%B", "main~1"]);
    assert!(body.contains("Sheriff-Reverts: ") && body.contains("Main-red: https://example/runs/9") && body.contains("tests/app.test.sh"), "{body}");
    assert!(sheriff_record(&fixture, &head).starts_with("revert "), "{}", sheriff_record(&fixture, &head));
    let told: Vec<String> = fixture.fleet().into_iter().filter(|call| call.contains("sheriff")).collect();
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].starts_with("fleet send --from service:north-land session-lane-older north land: main went red") && told[0].contains("lane-older"), "{told:?}");
    assert_eq!(git(&fixture.origin, &["for-each-ref", "refs/heads/claude/"]), "");
}

#[test]
fn sheriff_reverts_nothing_when_the_failure_persists_without_each_lane() {
    let (mut fixture, parent) = landed_pair("#!/usr/bin/env bash\nexit 1\n");
    let head = fixture.main();
    request_revert(&fixture, &head, &parent);
    let later = fixture.lane("later", "c.txt", "c");
    fixture.enqueue(&later, None);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from older", "change from newer", "change from later"]);
    assert!(sheriff_record(&fixture, &head).starts_with("environment tests/app.test.sh"), "{}", sheriff_record(&fixture, &head));
    assert!(fixture.fleet().iter().all(|call| !call.contains("sheriff")));
}

#[test]
fn sheriff_reverts_nothing_once_main_moved_past_the_failing_commit() {
    let (mut fixture, parent) = landed_pair(SUITE);
    let head = fixture.main();
    let fix = fixture.lane("fix", "app.txt", "fixed\n");
    fixture.enqueue(&fix, None);
    assert!(fixture.serve().status.success());
    request_revert(&fixture, &head, &parent);
    assert!(fixture.serve().status.success());
    assert_eq!(fixture.subjects(), ["change from older", "change from newer", "change from fix"]);
    assert!(sheriff_record(&fixture, &head).starts_with("stale main moved past"), "{}", sheriff_record(&fixture, &head));
}
