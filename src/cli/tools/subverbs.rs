use crate::landing::{fails_first, land_actions, lane_squash, lane_target, land_test_runner, red_check};
use crate::land_abandon;

pub(super) type Run = fn(&[String]) -> u8;

pub(super) fn runner(table: &[(&str, Run)], name: &str) -> Option<Run> {
    table.iter().find(|(known, _)| *known == name).map(|(_, run)| *run)
}

pub(super) fn graph(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "ready" => north_graph_ground::ready_command(rest),
        _ => north_graph_ingest::run(arguments),
    }
}

pub(super) fn code_health(arguments: &[String]) -> u8 {
    match arguments.first().map(String::as_str) {
        Some("build-time") => crate::build_time::run(&arguments[1..]),
        _ => crate::code_health::run(arguments),
    }
}

pub(super) fn ledger_verb(verb: &str, arguments: &[String]) -> u8 {
    let words: Vec<String> = std::iter::once(verb.to_owned()).chain(arguments.iter().cloned()).collect();
    crate::threads::client::run(&words, std::sync::Arc::new(crate::threads::outside::Unread))
}

const LAND: &[(&str, Run)] = &[
    ("abandon", |arguments| land_abandon::abandon(arguments, &super::super::abandon_witness::FleetWitness)),
    ("actions", land_actions::run),
    ("clean", lane_target::run),
    ("fails-first", fails_first::run),
    ("red-check", red_check::run),
    ("tests", land_test_runner::run),
    ("train", |arguments| ledger_verb("land-train", arguments)),
];

pub(super) fn land(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if let Some(run) = runner(LAND, verb) => return run(rest),
        Some((verb, rest)) if verb == "doctor" => return north_lander_doctor::run(rest),
        Some((verb, rest)) if verb == "drill" => return north_lander_doctor::drill::run(rest),
        Some((verb, rest)) if verb == "gate-stats" => return north_lander_doctor::gate_stats::run(rest),
        _ => {}
    }
    crate::land::run(arguments)
}

const LANE: &[(&str, Run)] = &[
    ("gc", crate::lanes::lane_gc),
    ("layout", crate::lanes::layout_check),
    ("reap", crate::lanes::wt_reap),
    ("reclaim", crate::ops::reclaim::run),
    ("rescue", crate::lanes::wt_rescue),
    ("seed", crate::lanes::target_seed),
    ("squash", lane_squash::run),
    ("sweep", crate::ops::cleanup_sweep::run),
];

pub(super) fn lane(arguments: &[String]) -> u8 {
    match arguments.split_first().and_then(|(verb, rest)| runner(LANE, verb).map(|run| run(rest))) {
        Some(code) => code,
        None => {
            let verbs: Vec<&str> = LANE.iter().map(|(verb, _)| *verb).collect();
            eprintln!("usage: north lane ({}) ...; north lane --help lists each", verbs.join("|"));
            2
        }
    }
}

const LEDGER: &[(&str, Run)] = &[
    ("flow", crate::flow::run),
    ("incident", |arguments| ledger_verb("incident", arguments)),
    ("scoreboard", super::super::scoreboard::run),
    ("tempo", crate::tempo::run),
    ("worker", super::super::worker_ledger::run),
];

pub(in crate::cli) fn ledger_runner(verb: &str) -> Option<Run> {
    runner(LEDGER, verb)
}

const RUNTIME: &[(&str, Run)] = &[
    ("agents", agent_runtime_update),
    ("agy", crate::ops::vendor_runtime::run_agy),
    ("claude", crate::ops::vendor_runtime::run_claude),
];

fn agent_runtime_update(arguments: &[String]) -> u8 {
    crate::timers::Timer::AgentRuntimeUpdate.run(arguments, &super::super::agents::claude_access())
}

pub(super) fn runtime(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if let Some(run) = runner(RUNTIME, verb) => run(rest),
        _ => crate::runtime::run(arguments),
    }
}
