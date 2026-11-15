use north_foundation::test_inputs::git::git_in as git;
use std::fs;
use std::path::Path;

fn commit(repo: &Path, message: &str) -> String {
    git(repo, &["add", "--all"]);
    git(repo, &["commit", "-q", "-m", message]);
    git(repo, &["rev-parse", "HEAD"])
}

#[test]
fn a_trust_boundary_lane_with_no_accept_is_judged_before_landing() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    git(repo, &["init", "-q", "-b", "main"]);
    fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.safe-push"), repo.join(".safe-push")).unwrap();
    let base = commit(repo, "main carries north's land lines");
    fs::create_dir_all(repo.join("crates/north-runtime/src/worker_sandbox")).unwrap();
    fs::write(repo.join("crates/north-runtime/src/worker_sandbox/grant.rs"), "pub fn grant() {}\n").unwrap();
    let head = commit(repo, "widen a worker sandbox grant");
    let queued = super::queue_commands(repo, &base, &head).unwrap();
    for scan in ["land scan review-accept", "land scan safe-push-keys"] {
        assert!(queued.iter().any(|command| command.contains(scan)), "a trust-boundary lane runs `{scan}` among its checks before queueing, which every road to the lander runs: {queued:#?}");
    }
}

#[test]
fn a_lane_whose_own_safe_push_pair_is_cut_by_a_comment_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    git(repo, &["init", "-q", "-b", "main"]);
    fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.safe-push"), repo.join(".safe-push")).unwrap();
    fs::create_dir_all(repo.join("harness")).unwrap();
    fs::write(repo.join("harness/gates.toml"), "[gate.safe-push-keys]\nmode = \"enforce\"\n").unwrap();
    let base = commit(repo, "main enforces safe-push-keys");
    let mut config = fs::read_to_string(repo.join(".safe-push")).unwrap();
    config.push_str("[land]\n\tlocal-only = no build waits (north#1247) :: true\n");
    fs::write(repo.join(".safe-push"), config).unwrap();
    let head = commit(repo, "add a local-only reason holding a comment character");
    let refusal = super::queue_commands(repo, &base, &head).unwrap_err();
    assert!(refusal.contains("invalid land.local-only"), "a lane whose own .safe-push pair git config cut at a # was judged by the base's lines and landed unparsed (north#1634, 2026-11-15): {refusal}");
}

#[test]
fn no_verdict_names_its_failing_step() {
    let dir = tempfile::tempdir().unwrap();
    let (log, outcome) = (dir.path().join("check.log"), dir.path().join("outcome.tsv"));
    let gave_up = "head-north: refused, no whole runtime at /home/tom/.local/state/north/runtimes/2091ad2751 after waiting, and an offload slot never builds north from the tree it checks";
    fs::write(&log, format!("error: an earlier warning that is not the reason\n{gave_up}\n{{\"decision\":\"COMMAND_EXITED\",\"exitCode\":75}}\n")).unwrap();
    fs::write(&outcome, format!("failed-step\t./bin/head-north --code src/cli/ land lint\t{}\n", log.display())).unwrap();
    let status = north_foundation::foundation::exit::NO_VERDICT_STATUS;
    let named = format!("{:?}", super::refusal(status, (dir.path(), "", ""), super::failing_step(&outcome, status)));
    assert!(named.contains("land lint") && named.contains(gave_up) && !named.contains("host or its connection"), "a check that gave up inside the slot was blamed on the host or its connection instead of naming itself and its own last line (north#1578, 2026-11-15): {named}");
    let dropped = format!("{:?}", super::refusal(status, (dir.path(), "", ""), super::failing_step(&dir.path().join("no-outcome"), status)));
    assert!(dropped.contains("host or its connection"), "a run whose connection dropped before any check reported was not named as one: {dropped}");
}
