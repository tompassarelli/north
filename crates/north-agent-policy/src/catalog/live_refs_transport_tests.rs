use std::path::Path;
use std::process::Command;

use super::*;
use super::super::live_refs::HookFact;

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
