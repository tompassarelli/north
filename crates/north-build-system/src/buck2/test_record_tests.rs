use super::{command, logged, runs};

/// `buck2 log show` of Buck2 2026-04-15 on gex45, 2026-11-12: a test run locally, one run on remote execution, and one answered from the action cache; the local command's environment is emptied.
const RECORDED: &str = include_str!("test-record.recorded.jsonl");

#[test]
fn a_buck2_test_record_names_each_test_binary_s_action_and_only_a_cache_hit_is_reused() {
    let read = runs(&mut RECORDED.as_bytes()).unwrap();

    let seen: Vec<_> = read.iter().map(|run| (run.target.as_str(), run.action.as_deref(), run.reused, run.passed, run.seconds)).collect();
    assert_eq!(
        seen,
        [
            ("root//crates/north-test-cases:north-test-cases-unittest", Some("ac4657fe33ae1bf44568534e6d86711d5a81f3304a4d6141a3f5c98c08c3136d:147"), false, true, 0.004_703),
            ("root//crates/north-build-system:north-build-system-unittest", Some("bcd47a7d6641c04ea0d0ede85c0e8f13a61422ef8a7ea9f98dd8505f39d9843d:147"), false, true, 0.156_35),
            ("root//crates/north-git-locks:north-git-locks-unittest", Some("1e488da8e3d91beea1e0ac10266fd7c7c9176318a41e2343e029a859d5ca4836:147"), true, true, 1.129_35),
        ]
    );
    assert!(read[2].printed.contains("PASS [   1.123s] north-git-locks tests::worktrees_of_one_clone_fetching_a_moving_origin_at_once_all_succeed_north_1190"), "{}", read[2].printed);
    let line = format!("./bin/land-shell --dev -- {}", logged(["buck2", "--isolation-dir", "north-g3", "test", "//..."].map(str::to_owned).to_vec()).join(" "));
    assert_eq!(command(&line).as_deref(), Some("./bin/land-shell --dev -- buck2 --isolation-dir north-g3 log show buck-out/north/test-runs/38ec3c232d1e9466.pb.zst"), "the record is read from the event log the run wrote, under the daemon of the generation it used, never from the newest command in the checkout");
    assert_eq!(command("./bin/land-shell --dev -- buck2 test //..."), None, "a run that names no event log has no record of its own");
    assert_eq!(command("./bin/land-shell --dev -- buck2 --isolation-dir north-g3 build //..."), None);
    assert_eq!(command("cargo nextest run --workspace"), None);
}
