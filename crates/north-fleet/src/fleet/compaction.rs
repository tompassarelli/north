//! The auto-compact window a session launches with, read from its brief's `Arm: context-NNNk` stamp.

const ARM: &str = "Arm: context-";

/// The `Arm:` stamp of a box launched with an auto-compact window of `window` tokens.
pub fn arm(window: u64) -> String {
    format!("context-{}k", window / 1000)
}

/// The window the brief's `Arm: context-NNNk` line names, in tokens.
pub fn window(brief: &str) -> Option<u64> {
    brief.lines().find_map(|line| line.trim().strip_prefix(ARM)?.strip_suffix('k')?.parse::<u64>().ok()).map(|thousands| thousands * 1000)
}
