pub mod agent_contain;
pub mod agents;
pub mod check;
pub mod citation_check;
pub mod context_audit;
pub mod convo;
pub mod detectors;
pub mod dispatch;
pub mod dividend;
pub mod docs;
pub mod experiments;
pub mod features_index;
pub mod flow;
pub mod incident_mine;
pub mod build_offload;
pub mod land_offload;
pub mod defaults_measure;
pub mod laptop;
pub mod laptop_ntfy;
pub mod laptop_runner;
pub mod job_watch;
pub mod model_watch;
pub mod nexus_ram;
pub mod observation_boxes;
pub mod outside_watch;
pub mod runtime_access;
pub mod private_check;
pub mod pacing;
pub mod private_desktop;
pub mod reclaim;
pub mod retention;
pub mod provider_capacity;
pub mod rationale_index;
pub mod resident_cap;
pub mod report;
#[cfg_attr(not(test), allow(dead_code))]
pub mod policy_replay;
pub mod runtime;
pub mod tune;
pub mod timers;
pub mod usage_web_read;
pub mod watch;

pub fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((command, rest)) if command == "dividend" => dividend::run(rest),
        Some((command, _)) if command == "check" => check::run(arguments),
        _ => {
            eprintln!("usage: north ops dividend [--dry-run] [CHECK] | north ops check [--dry-run]");
            2
        }
    }
}
