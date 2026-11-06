use std::path::Path;
use std::process::Command;

use super::gate_shadow::findings;

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim_end().to_owned()
}

fn commit(dir: &Path, files: &[(&str, &str)]) -> String {
    for (path, text) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    git(dir, &["add", "-A"]);
    git(dir, &["commit", "-qm", "lane"]);
    git(dir, &["rev-parse", "HEAD"])
}

const GATE: &str = "crates/north-guard/src/guard/launch_cap.rs";

#[test]
fn a_lane_adding_an_enforcing_gate_with_no_shadow_record_is_refused_and_its_shadow_twin_is_not() {
    let scratch = tempfile::tempdir().unwrap();
    let dir = scratch.path();
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["config", "user.name", "gate-shadow-test"]);
    git(dir, &["config", "user.email", "gate-shadow-test@example.invalid"]);
    let base = commit(dir, &[("README.md", "repo\n")]);
    let enforcing = commit(dir, &[(GATE, "pub fn refuse() {}\n"), ("harness/gates.toml", "[[gate]]\nid = \"launch-cap\"\nfiles = [\"crates/north-guard/src/guard/launch_cap.rs\"]\nmode = \"enforce\"\n")]);
    let refused = findings(dir, &base, &enforcing, 1_800_000_000).unwrap();
    assert!(refused.iter().any(|reason| reason.starts_with("launch-cap enforces without its shadow record")), "{refused:?}");
    git(dir, &["reset", "-q", "--hard", &base]);
    let unregistered = commit(dir, &[(GATE, "pub fn refuse() {}\n")]);
    let refused = findings(dir, &base, &unregistered, 1_800_000_000).unwrap();
    assert!(refused.iter().any(|reason| reason.starts_with(GATE)), "{refused:?}");
    git(dir, &["reset", "-q", "--hard", &base]);
    let shadowed = commit(dir, &[(GATE, "pub fn refuse() {}\n"), ("harness/gates.toml", "[[gate]]\nid = \"launch-cap\"\nfiles = [\"crates/north-guard/src/guard/launch_cap.rs\"]\nmode = \"shadow\"\n")]);
    assert_eq!(findings(dir, &base, &shadowed, 1_800_000_000).unwrap(), Vec::<String>::new());
}
