use std::path::Path;

const ARM: &str = "Arm: handoff-";
const REPEAT_STEP: u64 = 50_000;
pub const NO_HANDOFF: &str = "no-handoff";

pub fn arm(threshold: u64) -> String {
    format!("handoff-{}k", threshold / 1000)
}

pub fn threshold(brief: &str) -> Option<u64> {
    brief.lines().find_map(|line| line.trim().strip_prefix(ARM)?.strip_suffix('k')?.parse::<u64>().ok()).map(|thousands| thousands * 1000)
}

pub fn due(threshold: u64, context: u64, last: Option<u64>) -> bool {
    context >= threshold && last.is_none_or(|last| context >= last + REPEAT_STEP)
}

pub fn nudge(context: u64, threshold: u64, note: &Path) -> String {
    let note = note.display();
    format!(
        "Your context is {}k tokens, past the {}k handoff point of your brief's arm. Land anything that passes now; if work remains, \
         finish the step you are on, then write a handoff note to {note} with: the brief's goal and Done when list with each box's status; \
         the worktree, branch and commits; running background jobs with their output paths; the next step; evidence paths. \
         Then end your turn with `Not done:`, `Stop: leash` and the note's path.",
        context / 1000,
        threshold / 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_brief_on_a_handoff_arm_carries_a_threshold() {
        let brief = |arm: &str| format!("Item: north#1 x\nTier: worker\nArm: {arm}\nArm: sonnet-high\nDone when: y");
        assert_eq!(threshold(&brief(&arm(350_000))), Some(350_000));
        assert_eq!(threshold(&brief(NO_HANDOFF)), None);
        assert_eq!(threshold("Item: north#1 x\nTier: worker"), None);
    }

    #[test]
    fn the_experiment_arm_hands_off_at_150k_while_the_hook_and_watchdog_point_stays_350k() {
        use crate::pace::limits::{handoff_arm_ctx, handoff_ctx};
        let label = arm(handoff_arm_ctx());
        assert_eq!(label, "handoff-150k");
        assert_eq!(threshold(&format!("Tier: sonnet\nArm: {label}")), Some(150_000));
        assert!(due(150_000, 150_000, None) && !due(150_000, 149_999, None));
        assert_eq!(handoff_ctx(), 350_000);
        let watchdog = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../bin/capacity-watchdog")).unwrap();
        let hook = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/guard/worker_handoff.rs")).unwrap();
        assert!(watchdog.contains("DEFAULTS[\"handoff_ctx\"]") && hook.contains("limits::handoff_ctx()"));
    }

    #[test]
    fn the_nudge_fires_at_the_threshold_and_again_only_after_another_50k() {
        assert!(!due(350_000, 349_999, None));
        assert!(due(350_000, 350_000, None));
        assert!(!due(350_000, 399_999, Some(350_000)));
        assert!(due(350_000, 400_000, Some(350_000)));
    }
}
