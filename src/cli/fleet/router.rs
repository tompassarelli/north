use std::process::ExitCode;
use std::sync::Arc;

use crate::error::{NorthError, NorthResult};
use super::claims::{self, SWEEP_VERB, ThreadsCli, release_exited_claims};
use crate::fleet::model::ProviderName;
use crate::fleet::registry::{self, Controlled, Fleet};
use crate::fleet::store::ControlVerb;
use crate::fleet::roster::Roster;
use crate::fleet::runtimes::Runtimes;
use crate::providers::claude::fleet::Claude;
use crate::providers::codex::fleet::Codex;
use crate::providers::opencode::Deepseek;
use crate::providers::pi::PI;
use crate::providers::standin::Standin;
use crate::fleet::session_host::{FrameWatch, HostConfig};

use super::{desktop, interactive};
use super::adopt;
use super::ask;
use super::ledger::{LedgerLeases, LedgerRegistrar};
use super::row;
use super::show;
use crate::cli::{gate, orders, send};

pub fn run(arguments: &[String]) -> ExitCode {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let result = open_fleet_claiming(ThreadsCli::for_verb(arguments), &words).and_then(|fleet| {
        crate::cli::gate_closing::close_moot(&fleet);
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
    Runtimes::new(vec![Arc::new(Claude), Arc::new(Codex), Arc::new(Deepseek::opencode().with_lanes(lane_maker())), Arc::new(Deepseek::live(&PI).with_lanes(lane_maker())), Arc::new(Standin)])
}

pub(in crate::cli) fn roster() -> NorthResult<Roster> {
    registry::roster(&runtimes())
}

pub(in crate::cli) fn account_home(provider: &str, label: &str) -> Option<std::path::PathBuf> {
    roster().ok()?.account(&ProviderName::new(provider), label).ok().map(|account| account.home)
}

pub(in crate::cli) fn open_fleet() -> NorthResult<Fleet> {
    open_fleet_claiming(ThreadsCli::new(), &[])
}

fn open_fleet_claiming(claims: ThreadsCli, words: &[&str]) -> NorthResult<Fleet> {
    let ledger = send::ledger_for(words)?;
    Fleet::open(runtimes(), Arc::new(ledger), Arc::new(claims), Arc::new(LedgerRegistrar::new(|_| false)), Arc::new(LedgerLeases)).map(|fleet| fleet.writing_usage(Arc::new(usage_ingest_of())).recording_runs(Arc::new(run_ingest_of())).showing_coordinator_on(Arc::new(desktop::Desktop)).following_rewrites(Arc::new(super::rewrites::Recorded)))
}

fn usage_ingest_of() -> super::usage_ingest::IngestUsage {
    super::usage_ingest::IngestUsage::default()
}

fn run_ingest_of() -> super::run_ingest::IngestRuns {
    super::run_ingest::IngestRuns
}

pub fn host_watch(config: &HostConfig) -> Option<FrameWatch> {
    let provider = ProviderName::new(config.provider.as_deref()?);
    match open_fleet().and_then(|fleet| fleet.watch(&provider, &config.state_dir)) {
        Ok(watch) => Some(watch.watching()),
        Err(error) => {
            eprintln!("north session-host: turn watch unavailable: {}", error.user_message());
            None
        }
    }
}

fn dispatch(fleet: Fleet, arguments: &[String]) -> NorthResult<()> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => Err(NorthError::Usage(super::usage::USAGE.into())),
        ["hold", rest @ ..] => claims::hold(&fleet, rest),
        ["digest", ..] => {
            eprintln!("north fleet digest moved to `north work --since DURATION`; it prints the same landed, in-flight, gate, stall and held sections");
            Ok(())
        }
        ["adopt", rest @ ..] => adopt::run(&fleet, rest),
        [verb @ ("row" | "spool"), rest @ ..] => row::run(&fleet, verb, rest),
        ["send", rest @ ..] => send::run(&fleet, rest),
        ["ask", name, rest @ ..] => ask::run(&fleet, name, rest),
        ["orders", rest @ ..] => orders::run(&fleet, rest),
        ["gate", rest @ ..] => gate::run(&fleet, rest),
        words => super::views::run(&fleet, words).unwrap_or_else(|| dispatch_session(&fleet, words)),
    }
}

fn dispatch_session(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    match words {
        ["interactive", name] => interactive::run(fleet, name, false),
        ["interactive", name, interactive::AFTER_TURN] => interactive::run(fleet, name, true),
        ["forget", name, flags @ ..] if !flags.is_empty() => {
            let (force, successor) = super::forget_flags::parse(flags).ok_or_else(|| NorthError::Usage(super::usage::USAGE.into()))?;
            forget(fleet, name, force, successor)
        }
        [SWEEP_VERB] => claims::sweep(fleet),
        [verb, name] => session_verb(fleet, verb, name),
        ["show", name, flag] => show::run(fleet, name, flag),
        _ => Err(NorthError::Usage(super::usage::USAGE.into())),
    }
}

pub(super) fn session_verb(fleet: &Fleet, verb: &str, name: &str) -> NorthResult<()> {
    match verb {
        "interrupt" => controlled(fleet, ControlVerb::Interrupt, name),
        "stop" => controlled(fleet, ControlVerb::Stop, name),
        "forget" => forget(fleet, name, None, None),
        "show" => show::show(fleet, name, false),
        "watchdog" => fleet.guard(name),
        _ => Err(NorthError::Usage(super::usage::USAGE.into())),
    }
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
        Controlled::Withdrawn => println!("withdrew the queued launch {name}"),
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

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
