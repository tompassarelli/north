use crate::landing::{fails_first, flake, land_actions, land_dispatch, lane_squash, lane_target, land_test_runner, missed, red_check};
use north_mutants::mutant_runs;
use crate::land_abandon;

pub(super) type Run = fn(&[String]) -> u8;

pub(super) fn runner(table: &[(&str, Run)], name: &str) -> Option<Run> {
    table.iter().find(|(known, _)| *known == name).map(|(_, run)| *run)
}

pub(super) fn code_health(arguments: &[String]) -> u8 {
    match arguments.first().map(String::as_str) {
        Some("build-time") => north_code_health::build_time::run(&arguments[1..]),
        Some("split") => north_code_health::split::run(&arguments[1..]),
        _ => crate::code_health::run(arguments),
    }
}

fn opencode(arguments: &[String]) -> u8 {
    let value = ledger_verb("opencode", arguments);
    if value == 2 {
        return value;
    }
    println!();
    value.max(crate::providers::opencode::report::run(arguments))
}

pub(super) fn ledger_verb(verb: &str, arguments: &[String]) -> u8 {
    let words: Vec<String> = std::iter::once(verb.to_owned()).chain(arguments.iter().cloned()).collect();
    crate::threads::client::run(&words, std::sync::Arc::new(crate::threads::outside::Unread), crate::cli::THREADS_FAMILIES)
}

fn fails_first_runs(arguments: &[String]) -> u8 {
    let [runs, names @ ..] = arguments else {
        eprintln!("usage: north land fails-first-runs RUNS NAME...");
        return 64;
    };
    let Some(runs) = runs.parse::<usize>().ok().filter(|runs| *runs >= 2) else {
        eprintln!("usage: north land fails-first-runs RUNS NAME...: RUNS must be a number of at least 2, not {runs}");
        return 64;
    };
    let names: std::collections::BTreeSet<String> = names.iter().cloned().collect();
    let Ok(cwd) = std::env::current_dir() else { return north_foundation::foundation::exit::NO_VERDICT };
    match flake::executables(&cwd, &mut std::io::stderr()) {
        Ok(executables) => match flake::verdicts(&cwd, &executables, &names, runs, &mut std::io::stderr()).and_then(|judged| flake::print(&judged, &mut std::io::stdout())) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("north land fails-first-runs: {error}");
                north_foundation::foundation::exit::NO_VERDICT
            }
        },
        Err(None) => {
            eprintln!("north land fails-first-runs: the tests do not build");
            flake::TESTS_DO_NOT_BUILD
        }
        Err(Some(why)) => {
            eprintln!("north land fails-first-runs: {why}");
            north_foundation::foundation::exit::NO_VERDICT
        }
    }
}

fn mutant_runs(arguments: &[String]) -> u8 {
    if !arguments.is_empty() {
        eprintln!("usage: north land mutant-runs (the names come from MUTANT_TESTS and the trees from MUTANT_TREES, one per line)");
        return 64;
    }
    let lines = |variable: &str| -> Vec<String> { std::env::var(variable).unwrap_or_default().lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect() };
    let names: std::collections::BTreeSet<String> = lines("MUTANT_TESTS").into_iter().collect();
    let trees: Vec<std::path::PathBuf> = lines("MUTANT_TREES").into_iter().map(std::path::PathBuf::from).collect();
    let Ok(cwd) = std::env::current_dir() else { return north_foundation::foundation::exit::NO_VERDICT };
    let target = std::env::var_os(north_landing_gates::landing::trees::TARGET_ENV).map(std::path::PathBuf::from).unwrap_or_else(|| cwd.join("target"));
    match mutant_runs::run(&trees, &names, &target.join(mutant_runs::SHARED), &mut std::io::stdout()) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("north land mutant-runs: {error}");
            north_foundation::foundation::exit::NO_VERDICT
        }
    }
}

const LAND: &[(&str, Run)] = &[
    ("adopt", super::super::land_adopt::run),
    ("abandon", |arguments| land_abandon::abandon(arguments, &super::super::abandon_witness::FleetWitness)),
    ("actions", land_actions::run),
    ("fails-first-runs", fails_first_runs),
    ("mutant-runs", mutant_runs),
    ("dispatch", land_dispatch::run),
    ("clean", lane_target::run),
    ("fails-first", fails_first::run),
    ("missed", missed::run),
    ("red-check", red_check::run),
    ("release", north_safe_push::entry::release),
    ("tests", land_test_runner::run),
    ("train", |arguments| ledger_verb("land-train", arguments)),
];

pub(super) fn land(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if let Some(run) = runner(LAND, verb) => return run(rest),
        Some((verb, rest)) if verb == "withdraw" => return outside_the_sandbox("land", "withdraw", crate::land::withdraw::run, rest),
        _ => {}
    }
    crate::land::run(arguments)
}

const LANE: &[(&str, Run)] = &[
    ("exec", |arguments| crate::lanes::lane_exec(arguments, crate::worker_sandbox::confined(), crate::worker_sandbox::run_built)),
    ("gc", |arguments| outside_the_sandbox("lane", "gc", crate::lanes::lane_gc, arguments)),
    ("land-draft", super::super::draft_road::land),
    ("layout", crate::lanes::layout_check),
    ("new", crate::lanes::new_lane),
    ("reap", crate::lanes::wt_reap),
    ("reclaim", north_reclaim::reclaim::run),
    ("remove", |arguments| outside_the_sandbox("lane", "remove", crate::lanes::remove, arguments)),
    ("rescue", crate::lanes::wt_rescue),
    ("restore", crate::lanes::lane_restore),
    ("restore-packed-refs", crate::worker_sandbox::restore_packed_refs),
    ("seed", |arguments| crate::lanes::target_seed(arguments, builds_with_buck2())),
    ("stray", |arguments| outside_the_sandbox("lane", "stray", crate::lanes::stray, arguments)),
    ("squash", lane_squash::run),
    ("sync", |arguments| outside_the_sandbox("lane", "sync", crate::agent_catalog::sync_gate::sync_checkouts, arguments)),
    ("unlock", |arguments| outside_the_sandbox("lane", "unlock", crate::lanes::unlock, arguments)),
];

const HOME: &[(&str, Run)] = &[("move", |arguments| outside_the_sandbox("home", "move", crate::lanes::home_move, arguments))];

fn builds_with_buck2() -> bool {
    crate::platform::orchestration::read().is_ok_and(|cfg| crate::platform::orchestration::builds_with_buck2(&cfg, &crate::platform::host::name()))
}

fn outside_the_sandbox(noun: &str, verb: &str, run: Run, arguments: &[String]) -> u8 {
    let words: Vec<String> = [noun, verb].into_iter().map(str::to_owned).chain(arguments.iter().cloned()).collect();
    match crate::worker_sandbox::reenter_outside(&words) {
        Some(code) => code,
        None => run(arguments),
    }
}

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

pub(super) fn home(arguments: &[String]) -> u8 {
    match arguments.split_first().and_then(|(verb, rest)| runner(HOME, verb).map(|run| run(rest))) {
        Some(code) => code,
        None => {
            let verbs: Vec<&str> = HOME.iter().map(|(verb, _)| *verb).collect();
            eprintln!("usage: north home ({}) ...; north home --help lists each", verbs.join("|"));
            2
        }
    }
}

const LEDGER: &[(&str, Run)] = &[
    ("incident", |arguments| ledger_verb("incident", arguments)),
    ("opencode", opencode),
    ("show", crate::cli::ledger_show::run),
    ("worker", super::super::worker_ledger::run),
];

pub(in crate::cli) fn ledger_runner(verb: &str) -> Option<Run> {
    runner(LEDGER, verb)
}

const RUNTIME: &[(&str, Run)] = &[
    ("accept", |arguments| north_runtime::worker_sandbox::hold_client::accept_command(arguments, north_offload::land_offload::settles_fleet_here())),
    ("agy", north_runtime::vendor_runtime::run_agy),
    ("claude", north_runtime::vendor_runtime::run_claude),
    ("host-op", north_runtime::worker_sandbox::host_op_client::command),
    ("hold", |arguments| north_runtime::worker_sandbox::hold_client::command(arguments, north_offload::land_offload::settles_fleet_here())),
    ("opencode", north_runtime::vendor_runtime::run_opencode),
    ("pi", north_runtime::vendor_runtime::run_pi),
];

pub(super) fn runtime(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if let Some(run) = runner(RUNTIME, verb) => run(rest),
        _ => crate::runtime::run(arguments),
    }
}

const OPS: &[(&str, Run)] = &[
    ("credentials", crate::credentials::run),
    ("github", ops_github),
];

pub(super) fn ops(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if let Some(run) = runner(OPS, verb) => run(rest),
        _ => {
            eprintln!("usage: north ops (credentials|github) ...");
            2
        }
    }
}

fn ops_github(arguments: &[String]) -> u8 {
    crate::gh_broker::github(arguments)
}
