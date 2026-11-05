pub mod activation;
pub mod launch_critical;
pub mod generated_policy;
pub mod model_identity;
pub mod paths;
pub mod desktop_action;
pub mod tripwire;
pub mod git_stash;
pub mod handoff_record;
pub mod file_skill;
#[cfg(test)]
mod file_skill_tests;
pub mod native_evidence;
pub mod frozen_topics;
pub mod issue_filing;
pub mod issue_labels;
pub mod blind_stage;
pub mod reviewer_cli;
pub mod gh_write;
pub mod public_text;
pub mod shell_call;
pub mod sops_decrypt;
mod git_invocation;
#[cfg(test)]
mod git_tests;

pub mod heavy_command;
pub mod scratch_log;
pub mod brief_skeleton;
pub mod poll_loop;
mod poll_loop_measured;
pub mod session_kill;
pub mod clipboard;
pub mod codex_conduct;
#[cfg(test)]
mod codex_conduct_tests;
pub mod native_item;
pub mod native_launch;
pub mod worker_wait;
pub mod commit_paths;
pub mod worker_repair;
pub mod lead_timer;
#[cfg(test)]
mod lead_timer_tests;
pub mod agent_cwd;
#[cfg(test)]
mod agent_cwd_tests;
pub mod worker_tier;
pub mod spawn_capacity;
#[cfg(test)]
mod spawn_capacity_tests;
mod spawn_state;
#[cfg(test)]
mod worker_tier_tests;
pub mod code_health_edit;
pub mod hook_events;
pub mod hook_rules;
pub mod hook_stats;
#[cfg(test)]
mod hook_stats_tests;
pub mod unlanded_work;
#[cfg(test)]
mod unlanded_work_tests;
pub mod worker_handoff;
pub mod quiet_phrase;
pub mod permission_offer;
#[cfg(test)]
mod quiet_phrase_tests;
#[cfg(test)]
mod clipboard_tests;
#[cfg(test)]
mod native_item_tests;
#[cfg(test)]
mod native_launch_tests;
#[cfg(test)]
mod trigger_matrix;
#[cfg(test)]
mod worker_wait_tests;

pub mod corpus_scan;
pub mod modern_search;
pub mod resource_safe_search;
mod search_words;
#[cfg(test)]
mod search_guard_tests;
#[cfg(test)]
mod worker_handoff_tests;
pub mod subagent_teardown;
#[cfg(test)]
mod subagent_teardown_tests;
