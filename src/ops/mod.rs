pub(crate) mod advisor_report;
pub(crate) mod agent_contain;
pub(crate) mod agents;
pub(crate) mod check;
pub(crate) mod citation_check;
pub(crate) mod convo;
pub(crate) mod dispatch;
pub(crate) mod dividend;
pub(crate) mod flow;
pub(crate) mod incident_mine;
pub(crate) mod build_offload;
pub(crate) mod land_offload;
pub(crate) mod defaults_measure;
pub(crate) mod laptop;
pub(crate) mod laptop_ntfy;
pub(crate) mod laptop_runner;
pub(crate) mod job_watch;
pub(crate) mod model_watch;
pub(crate) mod nexus_ram;
pub(crate) mod outside_watch;
pub(crate) mod pacing;
pub(crate) mod private_desktop;
pub(crate) mod reclaim;
pub(crate) mod retention;
pub(crate) mod resident_cap;
pub(crate) mod report;
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod policy_replay;
pub(crate) mod runtime;
pub(crate) mod tune;
pub(crate) mod timers;
pub(crate) mod usage_web_read;

pub(crate) fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((command, rest)) if command == "dividend" => dividend::run(rest),
        Some((command, _)) if command == "check" => check::run(arguments),
        _ => {
            eprintln!("usage: north ops dividend [--dry-run] [CHECK] | north ops check [--dry-run]");
            2
        }
    }
}
