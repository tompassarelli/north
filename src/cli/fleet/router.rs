use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use super::claims::{ThreadsCli, release_exited_claims};
use crate::fleet::model::ProviderName;
use crate::fleet::registry::{self, Controlled, Fleet};
use crate::fleet::store::ControlVerb;
use crate::fleet::roster::Roster;
use crate::fleet::runtime::SESSION_ENV;
use crate::fleet::runtimes::Runtimes;
use crate::pacing::placement::{self as pacing_placement, config};
use crate::providers::claude::fleet::Claude;
use crate::providers::codex::fleet::Codex;
use crate::ledger::Ledger;
use crate::fleet::session_host::{FrameWatch, HostConfig};
use crate::tui;

use super::placement::{self, Overrides};
use super::admission;
use super::digest;
use super::duplicate;
use super::ledger::{LedgerLeases, LedgerRegistrar};
use super::lineage;
use super::overlap;
use super::launch::{LaunchParts, launch_parts};
#[cfg(test)]
use super::launch::launch_request;
use super::list;
use super::usage::usage;
use super::interactive;
use super::relaunch;
use super::revive;
use super::row;
use super::show;
use super::spend;
use super::waits;
use super::tick;
use super::tier;
use super::tree;
use crate::cli::{gate, orders, send};

const DEFAULT_ASK_SECS: u64 = 120;

pub fn run(arguments: &[String]) -> ExitCode {
    let result = open_fleet().and_then(|fleet| {
        default_overdue_gates(&fleet);
        release_exited_claims(&fleet, arguments);
        dispatch(fleet, arguments)
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {}", error.user_message());
            ExitCode::FAILURE
        }
    }
}

fn default_overdue_gates(fleet: &Fleet) {
    let settled = match fleet.default_overdue_gates() {
        Ok(settled) => settled,
        Err(error) => return eprintln!("north gate: overdue defaults not applied: {}", error.user_message()),
    };
    for (id, outcome) in settled {
        if let Err(error) = outcome {
            eprintln!("north gate {id}: {}", error.user_message());
        }
    }
}

fn runtimes() -> Runtimes {
    Runtimes::new(vec![Arc::new(Claude), Arc::new(Codex)])
}

pub(in crate::cli) fn roster() -> NorthResult<Roster> {
    registry::roster(&runtimes())
}

pub(in crate::cli) fn open_fleet() -> NorthResult<Fleet> {
    Fleet::open(runtimes(), Arc::new(Ledger::open_default()?), Arc::new(ThreadsCli::new()), Arc::new(LedgerRegistrar::new(far_machine)), Arc::new(LedgerLeases))
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
        [] => tui::fleet::run(fleet),
        [verb @ ("list" | "stranded" | "rebooted"), rest @ ..] => list::list(&fleet, verb, rest),
        ["tree"] => tree::tree(&fleet),
        ["spend", rest @ ..] => spend::run(&fleet, rest),
        [verb @ ("wait" | "waits"), rest @ ..] => waits::run(&fleet, verb, rest),
        ["accounts"] => accounts(&fleet),
        ["launch", rest @ ..] => launch(&fleet, rest),
        [verb @ ("row" | "spool"), rest @ ..] => row::run(&fleet, verb, rest),
        ["tick", rest @ ..] => tick::run(&fleet, rest),
        ["send", rest @ ..] => send::run(&fleet, rest),
        ["ask", name, rest @ ..] => ask(&fleet, name, rest),
        ["orders", rest @ ..] => orders::run(&fleet, rest),
        ["gate", rest @ ..] => gate::run(&fleet, rest),
        ["relaunch", name, rest @ ..] => relaunch::run(&fleet, name, rest),
        ["interactive", name] => interactive::run(&fleet, name),
        ["digest", rest @ ..] => digest::run(&fleet, rest),
        [verb, name] => session_verb(&fleet, verb, name),
        ["revive", name, rest @ ..] => revive::revive_with(&fleet, name, rest),
        ["show", name, flag] => show::run(&fleet, name, flag),
        _ => Err(usage()),
    }
}

fn session_verb(fleet: &Fleet, verb: &str, name: &str) -> NorthResult<()> {
    match verb {
        "interrupt" => controlled(fleet, ControlVerb::Interrupt, name),
        "stop" => controlled(fleet, ControlVerb::Stop, name),
        "forget" => controlled(fleet, ControlVerb::Forget, name),
        "revive" if fleet.session(name).is_err() => controlled(fleet, ControlVerb::Revive, name),
        "revive" => revive::revive(fleet, name, None, None, Overrides::default()),
        "show" => show::show(fleet, name, false),
        "watchdog" => fleet.guard(name),
        _ => Err(usage()),
    }
}


fn controlled(fleet: &Fleet, verb: ControlVerb, name: &str) -> NorthResult<()> {
    if let Controlled::Spooled(row) = fleet.control(verb, name)? {
        println!("spooled CONTROL row {}: {} {name} applies on the host that holds it at its next tick", row.id, verb.label());
    }
    Ok(())
}


fn accounts(fleet: &Fleet) -> NorthResult<()> {
    let roster = fleet.roster();
    for account in &roster.accounts {
        println!("{}\t{}", account.provider.label(), account.label);
    }
    for omitted in roster.omitted() {
        eprintln!("omitted: {omitted}");
    }
    Ok(())
}

pub(super) fn launch(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let caller = std::env::var(SESSION_ENV).ok();
    fleet.admit_launch_from(caller.as_deref())?;
    let proxy = lineage::proxy_reachable(fleet, caller.as_deref(), words);
    let LaunchParts { mut request, mut overrides, wait, take_over, overlap_ok } = launch_parts(&fleet.providers(), words, caller, proxy)?;
    tier::admit_class(&request)?;
    let started = tier::apply(&mut request, &mut overrides)?;
    if !take_over {
        let tier = started.as_ref().map_or("worker", |start| start.route.tier.name());
        if let overlap::Outcome::Queued(said) = overlap::guard(fleet, &request, tier, overlap_ok.as_deref())? {
            println!("{said}");
            return Ok(());
        }
    }
    let waiting = admission::Waiting::chosen(wait, fleet, request.parent.as_deref());
    let mut placed = admission::admitted(&request.name, waiting, |first| placement::placed(fleet, &request, &overrides, first), |line| eprintln!("{line}"), |text| admission::tell_parent(fleet, request.parent.as_deref(), text))?;
    if let Some(account) = placed.account.take() {
        request.account = account;
    }
    (request.placement, request.capacity_need) = (overlap::noted(duplicate::noted(fleet, &request, &overrides, placed.note)?, overlap_ok.as_deref()), placed.need);
    request.machine = placed.machine.or(request.machine);
    let record = if take_over { fleet.take_over(&request)? } else { fleet.launch(&request)? };
    tier::recorded(started, &request, &record.name);
    println!("{}", record.name);
    Ok(())
}

fn ask(fleet: &Fleet, name: &str, words: &[&str]) -> NorthResult<()> {
    let (question, timeout) = match words {
        [question @ .., "--timeout", secs] => (question, secs.parse().map_err(|_| usage())?),
        question => (question, DEFAULT_ASK_SECS),
    };
    if question.is_empty() {
        return Err(usage());
    }
    let (flags, question) = send::leading_flags(question)?;
    if question.is_empty() {
        return Err(usage());
    }
    let from = send::signer(fleet, flags.from, name)?;
    match fleet.ask(&from, name, &question.join(" "), Duration::from_secs(timeout))? {
        Some(reply) => {
            println!("{reply}");
            Ok(())
        }
        None => Err(NorthError::Rejected(format!("{name} did not answer within {timeout} s"))),
    }
}

#[cfg(test)]
#[path = "router_tests.rs"]
mod tests;
