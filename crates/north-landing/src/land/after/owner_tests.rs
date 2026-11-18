use north_base::platform::records_mirror;

use super::{Cause, Reverted};

#[test]
fn a_revert_notice_names_the_failing_step_the_mirrored_log_and_how_to_bring_the_lane_back() {
    let state = records_mirror::state_home();
    let log = state.join("north/land-queue/q/logs/post-land-fedcba987654.log");
    let copy = state.join("north/records-mirror/north/land-queue/q/logs/post-land-fedcba987654.log").display().to_string();
    let reverted = Reverted { branch: "thing-lane", commit: "abcdef0123456789", revert: "sheriff-revert-abcdef012", cause: Cause::RedMain { red: "fedcba9876543210", failing: "step cargo clippy", log: &log } };
    let notice = reverted.notice();
    assert!(notice.contains("step cargo clippy") && notice.contains(&copy), "{notice}");
    assert!(notice.contains("git cherry-pick abcdef0123456789") && notice.ends_with("Follows: abcdef012"), "{notice}");
}
