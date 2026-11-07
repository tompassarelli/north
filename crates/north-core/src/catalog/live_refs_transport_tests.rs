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

#[test]
fn a_publish_that_loses_the_race_for_the_ref_rebuilds_on_the_winner_and_succeeds() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = tempfile::tempdir().expect("fixture");
    let remote = fixture.path().join("south.git");
    git_in(fixture.path(), &["init", "-q", "--bare", remote.to_str().expect("path")]);
    let clone = fixture.path().join("clone");
    clone_of(&remote, &clone);
    let hook = clone.join(".git/hooks/pre-push");
    let marker = clone.join("rival-done");
    let script = format!(
        "#!/bin/sh\n[ -e {marker} ] && exit 0\n: > {marker}\ntree=$(git hash-object -t tree -w /dev/null)\ngit push -q origin $(git commit-tree $tree -m rival):refs/north/host-facts/gex45\n",
        marker = marker.display()
    );
    std::fs::write(&hook, script).expect("hook");
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let commit = publish(&clone, "origin", &facts("gex45", 2_000, "mine.sh")).expect("a lost race is not a failure");
    let tip = text(git(&remote, &["rev-parse", "refs/north/host-facts/gex45"], None).expect("tip"));
    assert_eq!(tip, commit);
    let held: Facts = serde_json::from_str(&text(git(&remote, &["show", &format!("{tip}:facts.json")], None).expect("show"))).expect("facts");
    assert_eq!(held.recorded_at_ms, 2_000);
    assert_eq!(text(git(&remote, &["rev-list", "--count", &tip], None).expect("count")), "2");
}
