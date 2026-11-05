use std::path::Path;
use std::process::Command;

use super::*;
use super::super::live_refs::{HookFact, judge};

fn git_in(directory: &Path, arguments: &[&str]) {
    let status = Command::new("git").arg("-C").arg(directory).args(arguments).status().expect("git");
    assert!(status.success(), "git {arguments:?}");
}

fn facts(host: &str, recorded_at_ms: u64, hook: &str) -> Facts {
    Facts {
        schema: SCHEMA.into(),
        host: host.into(),
        recorded_at_ms,
        hooks: vec![HookFact { path: format!("/home/tom/.agents/hooks/{hook}"), registered_in: "/home/tom/.claude/settings.json".into() }],
        unit_exec_starts: Vec::new(),
    }
}

fn clone_of(remote: &Path, directory: &Path) {
    let status = Command::new("git").args(["clone", "-q"]).arg(remote).arg(directory).status().expect("clone");
    assert!(status.success());
}

#[test]
fn hosts_publish_facts_by_fast_forward_and_a_third_clone_reads_every_hosts_latest() {
    let fixture = tempfile::tempdir().expect("fixture");
    let remote = fixture.path().join("south.git");
    git_in(fixture.path(), &["init", "-q", "--bare", remote.to_str().expect("path")]);
    let (first, second, reader) = (fixture.path().join("a"), fixture.path().join("b"), fixture.path().join("c"));
    for clone in [&first, &second, &reader] {
        clone_of(&remote, clone);
    }
    let one = publish(&first, "origin", &facts("whiterabbit", 1_000, "prompt-grounding.sh")).expect("first publish");
    let two = publish(&first, "origin", &facts("whiterabbit", 2_000, "kept.sh")).expect("second publish");
    assert_ne!(one, two);
    git_in(&remote, &["merge-base", "--is-ancestor", &one, &two]);
    publish(&second, "origin", &facts("gex45", 2_500, "prompt-grounding.sh")).expect("other host");

    let all = fetch_all(&reader, "origin").expect("fetch");
    assert_eq!(all.iter().map(|f| (f.host.as_str(), f.recorded_at_ms)).collect::<Vec<_>>(), [("gex45", 2_500), ("whiterabbit", 2_000)]);
    let report = judge(&all, "prompt-grounding.sh", &["whiterabbit".into(), "gex45".into(), "nexus".into()], 3_000, 3_600_000);
    assert_eq!(report.naming.len(), 1, "{report:?}");
    assert!(report.naming[0].starts_with("gex45: "));
    assert_eq!(report.unknown, ["nexus (no facts recorded)"]);
}

#[test]
fn reading_a_remote_with_no_published_facts_yields_nothing() {
    let fixture = tempfile::tempdir().expect("fixture");
    let remote = fixture.path().join("south.git");
    git_in(fixture.path(), &["init", "-q", "--bare", remote.to_str().expect("path")]);
    let reader = fixture.path().join("c");
    clone_of(&remote, &reader);
    assert!(fetch_all(&reader, "origin").expect("fetch").is_empty());
}

#[test]
fn facts_are_never_published_to_or_read_from_a_remote_that_is_not_the_private_south_repository() {
    let fixture = tempfile::tempdir().expect("fixture");
    for name in ["north.git", "north", "wisp.git"] {
        let remote = fixture.path().join(name);
        git_in(fixture.path(), &["init", "-q", "--bare", remote.to_str().expect("path")]);
        let clone = fixture.path().join(format!("clone-{name}"));
        clone_of(&remote, &clone);
        let error = publish(&clone, "origin", &facts("gex45", 1_000, "kept.sh")).expect_err("must refuse").to_string();
        assert!(error.contains("must be the private south repository"), "{error}");
        assert!(fetch_all(&clone, "origin").is_err());
        git_in(&remote, &["for-each-ref", "--count=1", "refs/north/host-facts"]);
        assert!(Command::new("git").arg("-C").arg(&remote).args(["for-each-ref", "refs/north/host-facts"]).output().expect("refs").stdout.is_empty());
    }
}
