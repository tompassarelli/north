use std::process::ExitCode;
use std::sync::Arc;

use crate::error::{NorthError, NorthResult};
use super::claims::{self, SWEEP_VERB, ThreadsCli, release_exited_claims};
use crate::fleet::model::{ProviderName, Role};
use crate::fleet::registry::{self, Controlled, Fleet};
use crate::fleet::store::ControlVerb;
use crate::fleet::roster::Roster;
use crate::fleet::runtime::SESSION_ENV;
use crate::fleet::runtimes::Runtimes;
use crate::pacing::placement::{self as pacing_placement, config};
use crate::providers::claude::fleet::Claude;
use crate::providers::codex::fleet::Codex;
use crate::providers::opencode::Deepseek;
use crate::providers::pi::PI;
use crate::ledger::Ledger;
use crate::fleet::session_host::{FrameWatch, HostConfig};
use crate::tui;

use super::placement::Overrides;
use super::{admission, desktop, gated, one_proxy, ranked, spend_queue, tier};
use super::run_ingest::IngestRuns;
use super::usage_ingest::IngestUsage;
use super::accounts;
use super::adopt;
use super::ask;
use super::drain;
use super::ledger::{LedgerLeases, LedgerRegistrar};
use super::lineage;
use super::launch::{LaunchParts, launch_parts};
use super::list;
use super::usage::usage;
use super::interactive;
use super::relaunch;
use super::revive;
use super::row;
use super::show;
use super::need;
use super::spend;
use super::waits;
use super::tick;
use super::tree;
use crate::cli::{gate, orders, send};

pub fn run(arguments: &[String]) -> ExitCode {
    let result = open_fleet_claiming(ThreadsCli::for_verb(arguments)).and_then(|fleet| {
        crate::cli::gate_orphan::close_orphaned(&fleet);
        sweep_gates(&fleet);
        release_exited_claims(&fleet, arguments);
        dispatch(fleet, arguments)
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {}", error.user_message());
            ExitCode::from(error.exit_code())
        }
    }
}

fn sweep_gates(fleet: &Fleet) {
    let settled = match fleet.sweep_gates() {
        Ok(settled) => settled,
        Err(error) => return eprintln!("north gate: gate sweep failed: {}", error.user_message()),
    };
    for (id, outcome) in settled {
        if let Err(error) = outcome {
            eprintln!("north gate {id}: {}", error.user_message());
        }
    }
}

fn lane_maker() -> Arc<crate::providers::opencode::Making> {
    Arc::new(|lane, issue| north_lanes::lanes::create_lane(lane, issue, None).map(drop))
}

fn runtimes() -> Runtimes {
    Runtimes::new(vec![Arc::new(Claude), Arc::new(Codex), Arc::new(Deepseek::opencode().with_lanes(lane_maker())), Arc::new(Deepseek::live(&PI).with_lanes(lane_maker()))])
}

pub(in crate::cli) fn roster() -> NorthResult<Roster> {
    registry::roster(&runtimes())
}

pub(in crate::cli) fn account_home(provider: &str, label: &str) -> Option<std::path::PathBuf> {
    roster().ok()?.account(&ProviderName::new(provider), label).ok().map(|account| account.home)
}

pub(in crate::cli) fn open_fleet() -> NorthResult<Fleet> {
    open_fleet_claiming(ThreadsCli::new())
}

fn open_fleet_claiming(claims: ThreadsCli) -> NorthResult<Fleet> {
    Fleet::open(runtimes(), Arc::new(Ledger::open_default()?), Arc::new(claims), Arc::new(LedgerRegistrar::new(far_machine)), Arc::new(LedgerLeases)).map(|fleet| fleet.writing_usage(Arc::new(IngestUsage::default())).recording_runs(Arc::new(IngestRuns)))
}

fn far_machine(host: &str) -> bool {
    config().is_ok_and(|cfg| pacing_placement::gate::spool_pulled(&cfg, host))
}

pub fn host_watch(config: &HostConfig) -> Option<Box<dyn FrameWatch>> {
    let provider = ProviderName::new(config.provider.as_deref()?);
    match open_fleet().and_then(|fleet| fleet.watch(&provider, &config.state_dir)) {
        Ok(watch) => Some(Box::new(watch)),
        Err(error) => {
            eprintln!("north session-host: turn watch unavailable: {}", error.user_message());
            None
        }
    }
}


fn dispatch(fleet: Fleet, arguments: &[String]) -> NorthResult<()> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => tui::fleet::run(fleet, crate::cli::dash::for_fleet, crate::cli::glance::lines),
        [verb @ ("list" | "stranded" | "rebooted" | "reports"), rest @ ..] => list::list(&fleet, verb, rest),
        ["tree"] => tree::tree(&fleet),
        ["spend", rest @ ..] => spend::run(&fleet, rest),
        [verb @ ("wait" | "waits"), rest @ ..] => waits::run(&fleet, verb, rest),
        ["need", rest @ ..] => need::run(rest),
        ["drain", rest @ ..] => drain::run(rest),
        ["hold", task] => hold(&fleet, task),
        ["accounts", flags @ ..] => accounts::run(&fleet, flags),
        ["digest", ..] => {
            eprintln!("north fleet digest moved to `north work --since DURATION`; it prints the same landed, in-flight, gate, stall and held sections");
            Ok(())
        }
        ["launch", rest @ ..] => launch(&fleet, rest),
        ["adopt", rest @ ..] => adopt::run(&fleet, rest),
        [verb @ ("row" | "spool"), rest @ ..] => row::run(&fleet, verb, rest),
        ["tick", rest @ ..] => tick::run(&fleet, rest),
        ["reap", rest @ ..] => waits::reap(&fleet, rest),
        ["send", rest @ ..] => send::run(&fleet, rest),
        ["ask", name, rest @ ..] => ask::run(&fleet, name, rest),
        ["orders", rest @ ..] => orders::run(&fleet, rest),
        ["gate", rest @ ..] => gate::run(&fleet, rest),
        _ => dispatch_session(&fleet, &words),
    }
}

fn dispatch_session(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    match words {
        ["relaunch", name, rest @ ..] => relaunch::run(fleet, name, rest),
        ["interactive", name] => interactive::run(fleet, name, false),
        ["interactive", name, interactive::AFTER_TURN] => interactive::run(fleet, name, true),
        ["forget", name, flags @ ..] if !flags.is_empty() => {
            let (force, successor) = super::forget_flags::parse(flags).ok_or_else(usage)?;
            forget(fleet, name, force, successor)
        }
        [SWEEP_VERB] => claims::sweep(fleet),
        [verb, name] => session_verb(fleet, verb, name),
        ["revive", name, rest @ ..] => revive::revive_with(fleet, name, rest),
        ["show", name, flag] => show::run(fleet, name, flag),
        _ => Err(usage()),
    }
}

pub(super) fn session_verb(fleet: &Fleet, verb: &str, name: &str) -> NorthResult<()> {
    match verb {
        "interrupt" => controlled(fleet, ControlVerb::Interrupt, name),
        "stop" => controlled(fleet, ControlVerb::Stop, name),
        "forget" => forget(fleet, name, None, None),
        "revive" if fleet.session(name).is_err() => controlled(fleet, ControlVerb::Revive, name),
        "revive" => revive::revive(fleet, name, None, None, Overrides::default()),
        "show" => show::show(fleet, name, false),
        "watchdog" => fleet.guard(name),
        _ => Err(usage()),
    }
}


fn hold(fleet: &Fleet, task: &str) -> NorthResult<()> {
    let session = std::env::var(SESSION_ENV).ok().filter(|name| !name.is_empty()).ok_or_else(|| NorthError::Usage(format!("north fleet hold REPO#N: ${SESSION_ENV} is unset, so no session holds it")))?;
    fleet.hold_task(&session, task)
}

fn controlled(fleet: &Fleet, verb: ControlVerb, name: &str) -> NorthResult<()> {
    told(fleet.control(verb, name)?, verb, name);
    Ok(())
}

fn told(controlled: Controlled, verb: ControlVerb, name: &str) {
    match controlled {
        Controlled::Here => {}
        Controlled::Spooled(row) => println!("spooled CONTROL row {}: {} {name} applies on the host that holds it at its next tick", row.id, verb.label()),
        Controlled::Retired { host } => println!("retired {name} at once instead of spooling a {}: its host {host} is no longer in placement", verb.label()),
    }
}


fn forget(fleet: &Fleet, name: &str, force: Option<&str>, successor: Option<&str>) -> NorthResult<()> {
    let forgotten = fleet.forget_unless_queued(name, force, successor)?;
    for lane in &forgotten.overrode {
        eprintln!("forgot {name} with {} (branch {}) still queued: {}", lane.ticket.display(), lane.branch, force.unwrap_or_default());
    }
    told(forgotten.controlled, ControlVerb::Forget, name);
    Ok(())
}


pub(in crate::cli) fn fleet_load() -> NorthResult<std::collections::BTreeMap<String, i64>> {
    super::placement::machine_load(&open_fleet()?)
}

pub(super) fn launch(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let caller = std::env::var(SESSION_ENV).ok();
    fleet.admit_launch_from(caller.as_deref())?;
    let proxy = lineage::proxy_reachable(fleet, caller.as_deref(), words);
    let parts = launch_parts(&fleet.providers(), words, caller.clone(), proxy)?;
    let queued = std::env::var(super::tick::QUEUE_ENTRY).ok().filter(|name| !name.is_empty());
    let result = launch_prepared(fleet, parts, caller, queued.as_deref());
    if let Some(name) = queued.filter(|_| !matches!(&result, Err(NorthError::Saturated(_)))) {
        let _ = fleet.remove_queued(&name);
    }
    result
}

fn launch_prepared(fleet: &Fleet, parts: LaunchParts, caller: Option<String>, queued: Option<&str>) -> NorthResult<()> {
    let LaunchParts { mut request, mut overrides, wait, take_over, priority, words } = parts;
    if !(take_over && request.role() == Role::Proxy) {
        one_proxy::admit(fleet, caller.as_deref(), &format!("launch {}", request.name))?;
    }
    lineage::reader(fleet, &mut request, caller.as_deref())?;
    super::claude_only::admit(&request)?;
    ranked::admit(fleet, &request, caller.as_deref())?;
    tier::admit_class(&request)?;
    let started = tier::apply(&mut request, &mut overrides)?;
    let waiting = admission::Waiting::chosen(wait, fleet, request.parent.as_deref());
    let settled = admission::admitted(&request.name, waiting, |first| super::placement::placed(fleet, &request, started.as_ref().map_or(&[][..], |start| start.needs.as_slice()), &overrides, first), |line| eprintln!("{line}"), |text| admission::tell_parent(fleet, request.parent.as_deref(), text))?;
    let mut placed = match settled {
        Ok(placed) => placed,
        Err(super::placement::Refusal::Spend(why)) if queued.is_none() => {
            spend_queue::enqueue(fleet, spend_queue::Refused { request: &request, overrides: &overrides, take_over, priority, caller, words: &words, why: why.clone() })?;
            eprintln!("fleet launch: {} is queued at {} priority: {why}", request.name, priority.label());
            println!("queued {}", request.name);
            return Ok(());
        }
        Err(super::placement::Refusal::Spend(why)) => return Err(NorthError::Saturated(why)),
        Err(super::placement::Refusal::Saturated(why)) => return Err(NorthError::Saturated(why)),
        Err(refusal) => return Err(refusal.into()),
    };
    if let Some(account) = placed.account.take() {
        request.account = account;
    }
    if let Some(model) = placed.model.take() {
        request.model = Some(model);
        request.effort = placed.effort.or(request.effort);
    }
    gated::admit(fleet, &mut request)?;
    (request.placement, request.capacity_need) = (placed.note, placed.need);
    request.machine = placed.machine.or(request.machine);
    let tasks = request.tasks();
    if let Err(error) = fleet.release_exited_claims_on(&tasks) {
        eprintln!("north fleet: {} launches with its holder check left to its claim, because the ledger did not say who holds {}: {}", request.name, tasks.join(" "), error.user_message());
    }
    let record = if take_over { fleet.take_over(&request).inspect(|record| desktop::show_taken(fleet, record))? } else { fleet.launch(&request)? };
    tier::recorded(started, &request, &record.name);
    println!("{}", record.name);
    Ok(())
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
