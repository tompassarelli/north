pub mod launch_critical;
pub mod desktop_action;
pub mod tripwire;
pub mod git_stash;
pub mod native_evidence;
pub mod frozen_topics;
pub mod issue_labels;
pub mod blind_stage;
pub mod reviewer_cli;
pub mod shell_call;
mod git_invocation;
#[cfg(test)]
mod git_tests;

pub mod heavy_command;
pub mod session_kill;
