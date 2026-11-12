use std::fs;

use super::reclaim_dead_daemons;

/// The layout of Buck2 2026-04-15 (`home_buck_dir` and the daemon dir in app/buck2_common/src/invocation_roots.rs): `<buckd>/<project root>/<isolation dir>/buckd.pid`.
#[test]
fn a_removed_lane_s_buck2_daemon_state_is_reclaimed_and_a_live_lane_s_is_kept_whole() {
    let machine = tempfile::tempdir().unwrap();
    let (root, buckd) = (machine.path().join("root"), machine.path().join("home/.buck/buckd"));
    let live = root.join("code/north/worktrees/live");
    fs::create_dir_all(&live).unwrap();
    fs::write(live.join(".buckconfig"), "").unwrap();
    let state = |lane: &str, isolation: &str| {
        let dir = buckd.join("code/north/worktrees").join(lane).join(isolation);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("buckd.pid"), "1").unwrap();
        dir
    };
    let (kept, editor, removed) = (state("live", "v2"), state("live", ".rust-analyzer"), state("removed", "v2"));

    let would = reclaim_dead_daemons(&buckd, &root, true);
    assert_eq!(would.iter().map(|daemon| daemon.path.clone()).collect::<Vec<_>>(), [buckd.join("code/north/worktrees/removed")]);
    assert!(removed.join("buckd.pid").is_file(), "a dry run removed daemon state");

    reclaim_dead_daemons(&buckd, &root, false);
    assert!(!buckd.join("code/north/worktrees/removed").exists(), "the daemon state of a lane that is gone stayed, so its daemon keeps 0.5 to 0.9 GiB resident for good");
    assert!(kept.join("buckd.pid").is_file() && editor.join("buckd.pid").is_file(), "a live lane lost daemon state, which stops its daemon mid-build");
    assert!(reclaim_dead_daemons(&buckd, &root, false).is_empty());
}
