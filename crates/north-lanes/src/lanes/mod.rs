mod buck_daemons;
mod exec;
mod gc;
pub use gc::USAGE as GC_USAGE;
mod hardcoded_paths;
mod home_move;
mod landed_release;
mod landed_tickets;
mod lane_issue;
pub mod draft_mark;
mod lane_rejection;
mod layout_check;
mod live_process;
mod nested_shell;
mod orphan;
mod new;
mod pin_retire;
mod reap;
mod reject;
mod remove;
mod reference_search;
mod repo;
mod rescue;
mod rewritten;
mod stray;
mod target_seed;
mod target_subvolume;
mod unlock;
mod unpushed;
mod verdict;

pub use exec::{changed_locks, run as lane_exec};
pub use gc::{Inputs as LaneGcInputs, repo as repo_verb, run as lane_gc, run_in as lane_gc_in};
pub use hardcoded_paths::run as hardcoded_paths;
pub use landed_release::{ReleasedTarget, release_landed_targets};
pub use home_move::run as home_move;
pub use layout_check::run as layout_check;
pub use new::{create as create_lane, run as new_lane};
pub use pin_retire::run as pin_retire;
pub use reap::run as wt_reap;
pub use remove::run as remove;
pub use rescue::run as wt_rescue;
pub use stray::run as stray;
pub use buck_daemons::{DeadDaemon, reclaim_dead_daemons};
pub use target_seed::{DeadSeed, reclaim_dead_seeds, run as target_seed};
pub use unlock::run as unlock;

#[cfg(test)]
mod new_tests;
#[cfg(test)]
mod remove_tests;
#[cfg(test)]
mod reject_tests;
#[cfg(test)]
mod pin_retire_tests;
#[cfg(test)]
mod reap_tests;
#[cfg(test)]
mod rescue_tests;
#[cfg(test)]
mod stray_tests;
#[cfg(test)]
mod unpushed_tests;
#[cfg(test)]
mod verdict_tests;

#[cfg(test)]
const TEST_NOW_SECS: u64 = 4_102_444_800;

#[cfg(test)]
pub(crate) fn test_now() -> std::time::SystemTime {
    std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(TEST_NOW_SECS)
}

#[cfg(test)]
pub(crate) fn test_now_ms() -> i64 {
    i64::try_from(TEST_NOW_SECS * 1000).unwrap()
}

#[cfg(test)]
pub(crate) fn touched_at(tree: &std::path::Path, at: std::time::SystemTime) {
    let admin = std::path::PathBuf::from(north_base::test_inputs::git::git_in(tree, &["rev-parse", "--path-format=absolute", "--git-dir"]));
    for touched in [tree.to_path_buf(), admin.join("HEAD"), admin.join("index")] {
        if touched.exists() {
            std::fs::File::open(&touched).unwrap().set_modified(at).unwrap();
        }
    }
    for entry in std::fs::read_dir(tree).unwrap().flatten().filter(|entry| entry.file_name() != ".git") {
        if entry.path().is_file() {
            std::fs::File::options().write(true).open(entry.path()).unwrap().set_modified(at).unwrap();
        }
    }
}
