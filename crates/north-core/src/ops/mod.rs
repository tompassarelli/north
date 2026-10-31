pub mod advisor_report;
pub mod agent_contain;
pub mod agents;
pub mod check;
pub mod citation_check;
pub mod convo;
pub mod dispatch;
pub mod dividend;
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
pub mod outside_watch;
pub mod pacing;
pub mod private_desktop;
pub mod reclaim;
pub mod retention;
pub mod resident_cap;
pub mod report;
#[cfg_attr(not(test), allow(dead_code))]
pub mod policy_replay;
pub mod runtime;
pub mod tune;
pub mod timers;
pub mod usage_web_read;

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
