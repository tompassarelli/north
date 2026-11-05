use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::json;

use super::refuse_static_gates;

fn run(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(repo).args(["-c", "commit.gpgsign=false", "-c", "user.name=Gate fixture", "-c", "user.email=fixture@example.invalid"]).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn allow(repo: &Path, globs: &[&str]) {
    let entries: Vec<_> = globs.iter().map(|glob| json!({"glob":glob,"class":"script","gap":"fixture gap","provenance":"Tom 2026-10-15: permit this fixture","port_owner":"fixture-port-owner","port_target":"src/ported.rs","commitment":"Rust port by 2026-10-16 18:00","created":"2026-10-15"})).collect();
    fs::write(repo.join(".language-allowlist.json"), json!({"ratchet":["script"],"entries":entries}).to_string()).unwrap();
}

fn commit(repo: &Path, message: &str) -> String {
    run(repo, &["add", "--all"]);
    run(repo, &["commit", "-q", "-m", message]);
    run(repo, &["rev-parse", "HEAD"])
}

#[test]
fn a_lane_only_behind_main_is_judged_on_its_own_changes_not_on_what_main_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    run(repo, &["init", "-q", "-b", "main"]);
    allow(repo, &["tool.sh"]);
    let fork = commit(repo, "allow a script");
    allow(repo, &[]);
    let main = commit(repo, "main drops the allowance");
    run(repo, &["checkout", "-q", "-b", "lane", &fork]);
    fs::write(repo.join("notes.md"), "lane change\n").unwrap();
    commit(repo, "lane touches only notes");
    assert!(!crate::language_gate::check(repo, &main, "HEAD"), "diffing from main's tip shows main's removal as the lane widening the allowlist");
    assert_eq!(refuse_static_gates(repo, &main, "HEAD"), Ok(()));
}
