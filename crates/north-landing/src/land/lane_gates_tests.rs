use std::process::Command;

use super::*;

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(root).args(arguments).output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn commit(root: &Path, path: &str, text: &str) -> String {
    fs::write(root.join(path), text).unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", path]);
    git(root, &["rev-parse", "HEAD"])
}

fn gates(root: &Path, base: &str, head: &str, state_home: &Path, code_health: &Path) -> Vec<&'static str> {
    failed(&Lane { worktree: root, base, head, state_home, code_health }).map(|(gates, _)| gates).unwrap_or_default()
}

#[test]
fn a_lane_code_health_refuses_after_landing_is_refused_before_it_and_an_after_line_needs_a_check_or_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let (root, state_home) = (dir.path().join("repo"), dir.path().join("state"));
    fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    for (key, value) in [("user.name", "t"), ("user.email", "t@example.com"), ("commit.gpgsign", "false")] {
        git(&root, &["config", key, value]);
    }
    let stub = dir.path().join("code-health");
    north_base::foundation::shim::write_script(&stub, "grep -q complex triage.ts && exit 1\nexit 0\n");
    fs::write(root.join("structure.toml"), "").unwrap();
    let base = commit(&root, "triage.ts", "simple\n");
    let wisp_af6ed3fa = commit(&root, "triage.ts", "complex\n");
    assert_eq!(gates(&root, &base, &wisp_af6ed3fa, &state_home, &stub), ["code-health"]);
    let simple = commit(&root, "triage.ts", "simple again\n");
    let after = commit(&root, ".safe-push", "[land]\n\tafter = ** :: make slow-suite\n");
    assert_eq!(gates(&root, &simple, &after, &state_home, &stub), ["after-only reason"]);
    let reasoned = commit(&root, ".safe-push", "[land]\n\tafter = ** :: make slow-suite\n\tafter-only = reads the landed main's deployment :: make slow-suite\n");
    assert!(gates(&root, &simple, &reasoned, &state_home, &stub).is_empty());
}
