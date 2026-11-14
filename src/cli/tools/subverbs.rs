use crate::landing::{fails_first, flake, land_actions, land_dispatch, lane_squash, lane_target, land_test_runner, missed, red_check};
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
    let Ok(cwd) = std::env::current_dir() else { return north_base::foundation::exit::NO_VERDICT };
    let side = north_build_system::here();
    let built = side.test_build(&cwd).map_err(Some).and_then(|invocation| {
        let held = side.test_lock(&cwd).map_err(Some)?;
        let ran = invocation.command().stdin(std::process::Stdio::null()).output();
        drop(held);
        let output = ran.map_err(|error| Some(error.to_string()))?;
        let log = String::from_utf8_lossy(&output.stderr).into_owned();
        eprint!("{log}");
        if !output.status.success() {
            return Err(if side.broken(output.status.code(), &log) { None } else { Some(format!("the build exited {}", output.status)) });
        }
        side.test_executables(&cwd, &String::from_utf8_lossy(&output.stdout)).map_err(Some)
    });
    match built {
        Ok(executables) => {
            let executables: Vec<std::path::PathBuf> = executables.iter().map(|executable| cwd.join(executable)).collect();
            match flake::verdicts(&cwd, &executables, &names, runs, &mut std::io::stderr(), &mut std::io::stdout()) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("north land fails-first-runs: {error}");
                    north_base::foundation::exit::NO_VERDICT
                }
            }
        }
        Err(None) => {
            eprintln!("north land fails-first-runs: the tests do not build");
            flake::TESTS_DO_NOT_BUILD
        }
        Err(Some(why)) => {
            eprintln!("north land fails-first-runs: {why}");
            north_base::foundation::exit::NO_VERDICT
        }
    }
}

const LAND: &[(&str, Run)] = &[
    ("abandon", |arguments| land_abandon::abandon(arguments, &super::super::abandon_witness::FleetWitness)),
    ("actions", land_actions::run),
    ("fails-first-runs", fails_first_runs),
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
        Some((verb, rest)) if verb == "doctor" => return north_lander_doctor::run(rest, &review_holds),
        Some((verb, rest)) if verb == "drill" => return north_lander_doctor::drill::run(rest),
        Some((verb, rest)) if verb == "withdraw" => return outside_the_sandbox("land", "withdraw", crate::land::withdraw::run, rest),
        Some((verb, rest)) if verb == "gate-stats" => return north_lander_doctor::gate_stats::run(rest),
        Some((verb, rest)) if verb == "host-gate" => return north_lander_doctor::host_gate::run(rest),
        _ => {}
    }
    crate::land::run(arguments)
}

const LANE: &[(&str, Run)] = &[
    ("exec", |arguments| crate::lanes::lane_exec(arguments, crate::worker_sandbox::confined(), crate::worker_sandbox::run_built)),
    ("audit", |arguments| crate::lanes::repo_verb(&[vec!["audit".to_owned()], arguments.to_vec()].concat())),
    ("gc", |arguments| outside_the_sandbox("lane", "gc", crate::lanes::lane_gc, arguments)),
    ("land-draft", super::super::draft_road::land),
    ("layout", crate::lanes::layout_check),
    ("new", crate::lanes::new_lane),
    ("reap", crate::lanes::wt_reap),
    ("reclaim", north_reclaim::reclaim::run),
    ("remove", |arguments| outside_the_sandbox("lane", "remove", crate::lanes::remove, arguments)),
    ("rescue", crate::lanes::wt_rescue),
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
    ("flow", crate::flow::run),
    ("incident", |arguments| ledger_verb("incident", arguments)),
    ("opencode", opencode),
    ("show", crate::cli::ledger_show::run),
    ("tempo", crate::tempo::run),
    ("worker", super::super::worker_ledger::run),
];

pub(in crate::cli) fn ledger_runner(verb: &str) -> Option<Run> {
    runner(LEDGER, verb)
}

const RUNTIME: &[(&str, Run)] = &[
    ("accept", |arguments| north_runtime::worker_sandbox::hold_client::accept_command(arguments, north_offload::land_offload::settles_fleet_here())),
    ("agents", agent_runtime_update),
    ("agy", north_runtime::vendor_runtime::run_agy),
    ("claude", north_runtime::vendor_runtime::run_claude),
    ("eval-unit", north_runtime::worker_sandbox::eval_unit_client::command),
    ("hold", |arguments| north_runtime::worker_sandbox::hold_client::command(arguments, north_offload::land_offload::settles_fleet_here())),
    ("opencode", north_runtime::vendor_runtime::run_opencode),
    ("pi", north_runtime::vendor_runtime::run_pi),
];

fn agent_runtime_update(arguments: &[String]) -> u8 {
    crate::timers::runtime_update::run(arguments)
}

pub(super) fn runtime(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if let Some(run) = runner(RUNTIME, verb) => run(rest),
        _ => crate::runtime::run(arguments),
    }
}

const OPS: &[(&str, Run)] = &[
    ("credentials", crate::ops::credentials::run),
    ("github", ops_github),
    ("strategy", super::super::strategy::run),
    ("watch", ops_watch),
];

pub(super) fn ops(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if let Some(run) = runner(OPS, verb) => run(rest),
        Some((command, rest)) if command == "dividend" => north_expectations::dividend::run(rest),
        Some((command, _)) if command == "check" => north_expectations::check::run(arguments),
        _ => {
            eprintln!("usage: north ops dividend [--dry-run] [CHECK] | north ops check [--dry-run | --synthetic]");
            2
        }
    }
}

fn ops_github(arguments: &[String]) -> u8 {
    crate::ops::gh_broker::github(arguments)
}

fn ops_watch(arguments: &[String]) -> u8 {
    match crate::watch::run(arguments) {
        0 => north_expectations::watch::run(&["--once".to_owned()]),
        refused => refused,
    }
}

fn review_holds() -> Vec<north_lander_doctor::facts::ReviewHold> {
    let environment: Vec<(String, String)> = ["NORTH_VERDICT_CACHE_DIR", "XDG_STATE_HOME"].into_iter().filter_map(|name| Some((name.to_owned(), std::env::var(name).ok()?))).collect();
    let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let standing = north_runtime::runtime::review_hold::standing(&environment, &home).unwrap_or_default();
    standing.into_iter().map(|hold| north_lander_doctor::facts::ReviewHold { commit: hold.commit, session: hold.session, why: hold.why }).collect()
}
