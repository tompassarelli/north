use std::path::Path;

use super::*;

fn git_in(directory: &Path, arguments: &[&str]) {
    let status = std::process::Command::new("git")
        .current_dir(directory)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .args(arguments)
        .status()
        .unwrap();
    assert!(status.success(), "git {arguments:?}");
}

fn commit_registry(directory: &Path, text: &str) -> String {
    std::fs::create_dir_all(directory.join("harness")).unwrap();
    std::fs::write(directory.join(REGISTRY), text).unwrap();
    git_in(directory, &["add", REGISTRY]);
    git_in(directory, &["commit", "-qm", "registry"]);
    git::text(directory, &["rev-parse", "HEAD"]).unwrap().trim().to_owned()
}

fn run(directory: &Path, state: &Path, base: &str, head: &str) -> (u8, String) {
    let mut out = Vec::new();
    let status = scan(directory, &format!("{base}..{head}"), state, &mut out);
    (status, String::from_utf8(out).unwrap())
}

#[test]
fn a_lane_that_makes_a_gate_enforce_without_its_shadow_review_is_refused_and_a_shadow_scan_only_logs_it() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git_in(&repo, &["init", "-q", "-b", "main"]);
    let shadowing = "[gate.gate-shadow]\nmode = \"shadow\"\n[gate.privacy]\nmode = \"shadow\"\n";
    let base = commit_registry(&repo, shadowing);
    let state = root.path().join("state");

    let unreviewed = commit_registry(&repo, &format!("{shadowing}[gate.spend]\nmode = \"enforce\"\n"));
    let (status, text) = run(&repo, &state, &base, &unreviewed);
    assert_eq!((status, text.contains("spend")), (0, true), "{text}");
    let log = std::fs::read_to_string(state.join("north/gates/gate-shadow.jsonl")).unwrap();
    let row: serde_json::Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
    assert_eq!((row["would_refuse"].as_bool(), row["gates"][0].as_str()), (Some(true), Some("spend")), "{log}");

    let enforcing = "[gate.gate-shadow]\nmode = \"enforce\"\n[gate.gate-shadow.review]\nreviewed = \"2026-11-08\"\nby = \"tom\"\ndecisions = 20\n[gate.privacy]\nmode = \"shadow\"\n";
    let armed = commit_registry(&repo, enforcing);
    let (status, text) = run(&repo, &state, &unreviewed, &armed);
    assert_eq!(status, 0, "{text}");

    let switched = commit_registry(&repo, &format!("{enforcing}[gate.spend]\nmode = \"enforce\"\n"));
    let (status, text) = run(&repo, &state, &armed, &switched);
    assert_eq!((status, text.contains("refused")), (1, true), "{text}");

    let reviewed = commit_registry(&repo, &format!("{enforcing}[gate.spend]\nmode = \"enforce\"\n[gate.spend.review]\nreviewed = \"2026-11-08\"\nby = \"tom\"\nhours = 24\n"));
    let (status, text) = run(&repo, &state, &armed, &reviewed);
    assert_eq!(status, 0, "{text}");
}
