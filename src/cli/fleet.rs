use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};
use super::fleet_claims::{ThreadsCli, release_exited_claims};
use crate::fleet::model::{Entry, EntryKind, ProviderName, Status, Transcript};
use crate::fleet::registry::{self, Fleet, urgent_text};
use crate::fleet::roster::Roster;
use crate::fleet::runtime::SESSION_ENV;
use crate::fleet::runtimes::Runtimes;
use crate::providers::claude::fleet::Claude;
use crate::providers::codex::fleet::Codex;
use crate::fleet::turns::Turn;
use crate::ledger::Ledger;
use crate::fleet::session_host::{FrameWatch, HostConfig};
use crate::tui;

use super::fleet_placement::{self, Overrides};
use super::fleet_admission;
use super::fleet_duplicate;
use super::fleet_lineage;
use super::fleet_launch::launch_parts;
#[cfg(test)]
use super::fleet_launch::launch_request;
use super::fleet_list;
use super::fleet_usage::usage;
use super::fleet_revive;
use super::fleet_spend;
use super::fleet_waits;
use super::fleet_tier;
use super::fleet_tree;
use super::{gate, orders, relay};

const DEFAULT_ASK_SECS: u64 = 120;
const FOLLOW_POLL: Duration = Duration::from_millis(250);

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

pub(super) fn roster() -> NorthResult<Roster> {
    registry::roster(&runtimes())
}

pub(super) fn open_fleet() -> NorthResult<Fleet> {
    Fleet::open(runtimes(), Arc::new(Ledger::open_default()?), Arc::new(ThreadsCli))
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

fn turn_json(turn: &Turn) -> Value {
    json!({
        "exit": turn.exit.label(),
        "ended_ms": turn.ended_ms,
        "turns": turn.turns,
        "tokens_in": turn.tokens.input,
        "tokens_out": turn.tokens.output,
        "tokens_cache_read": turn.tokens.cache_read,
        "tokens_cache_write": turn.tokens.cache_write,
    })
}


fn dispatch(fleet: Fleet, arguments: &[String]) -> NorthResult<()> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => tui::fleet::run(fleet),
        [verb @ ("list" | "stranded" | "rebooted"), rest @ ..] => fleet_list::list(&fleet, verb, rest),
        ["tree"] => fleet_tree::tree(&fleet),
        ["spend", rest @ ..] => fleet_spend::run(&fleet, rest),
        [verb @ ("wait" | "waits"), rest @ ..] => fleet_waits::run(&fleet, verb, rest),
        ["accounts"] => accounts(&fleet),
        ["launch", rest @ ..] => launch(&fleet, rest),
        [verb @ ("send" | "relay" | "outbox"), rest @ ..] => relay::run(&fleet, verb, rest),
        ["ask", name, rest @ ..] => ask(&fleet, name, rest),
        ["orders", rest @ ..] => orders::run(&fleet, rest),
        ["gate", rest @ ..] => gate::run(&fleet, rest),
        [verb, name] => session_verb(&fleet, verb, name),
        ["revive", name, rest @ ..] => fleet_revive::revive_with(&fleet, name, rest),
        ["show", name, flag] => show_with(&fleet, name, flag),
        _ => Err(usage()),
    }
}

fn session_verb(fleet: &Fleet, verb: &str, name: &str) -> NorthResult<()> {
    match verb {
        "interrupt" => fleet.interrupt(name),
        "stop" => fleet.stop(name),
        "forget" => fleet.forget(name),
        "revive" => fleet_revive::revive(fleet, name, None, None, Overrides::default()),
        "show" => show(fleet, name, false),
        "watchdog" => fleet.guard(name),
        _ => Err(usage()),
    }
}


fn show_with(fleet: &Fleet, name: &str, flag: &str) -> NorthResult<()> {
    match flag {
        "--follow" => show(fleet, name, true),
        "--json" => show_json(fleet, name),
        _ => Err(usage()),
    }
}

fn show_json(fleet: &Fleet, name: &str) -> NorthResult<()> {
    let follower = fleet.follow(name)?;
    let transcript = follower.transcript();
    let entries: Vec<Value> = transcript.entries.iter().map(|entry| json!({"kind": entry.kind.label(), "text": entry.text})).collect();
    let mut session = fleet_list::session_json(follower.record(), transcript.status, &fleet.waits());
    session["active_turn"] = json!(transcript.active_turn);
    session["entries"] = Value::Array(entries);
    session["turns"] = transcript.turns.iter().map(turn_json).collect();
    println!("{session}");
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

fn launch(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let caller = std::env::var(SESSION_ENV).ok();
    fleet.admit_launch_from(caller.as_deref())?;
    let proxy = fleet_lineage::proxy_reachable(fleet, caller.as_deref(), words);
    let (mut request, mut overrides, wait) = launch_parts(&fleet.providers(), words, caller, proxy)?;
    fleet_tier::admit_class(&request)?;
    let started = fleet_tier::apply(&mut request, &mut overrides)?;
    let waiting = fleet_admission::Waiting::chosen(wait, fleet, request.parent.as_deref());
    let placed = fleet_admission::admitted(&request.name, waiting, |first| fleet_placement::placed(fleet, &request, &overrides, first), |line| eprintln!("{line}"), |text| fleet_admission::tell_parent(fleet, request.parent.as_deref(), text))?;
    (request.placement, request.capacity_need) = (fleet_duplicate::noted(fleet, &request, &overrides, placed.note)?, placed.need);
    request.machine = placed.machine.or(request.machine);
    let record = fleet.launch(&request)?;
    fleet_tier::recorded(started, &request, &record.name);
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
    let from = relay::signer(fleet, name);
    match fleet.ask(from.as_deref(), name, &question.join(" "), Duration::from_secs(timeout))? {
        Some(reply) => {
            println!("{reply}");
            Ok(())
        }
        None => Err(NorthError::Rejected(format!("{name} did not answer within {timeout} s"))),
    }
}

fn show(fleet: &Fleet, name: &str, follow: bool) -> NorthResult<()> {
    let mut follower = fleet.follow(name)?;
    let mut printed = print_entries(follower.transcript(), 0);
    println!("[{}]", follower.transcript().status.label());
    while follow && follower.transcript().status != Status::Exited {
        std::thread::sleep(FOLLOW_POLL);
        let before = follower.transcript().status;
        if follower.poll()? {
            printed = print_entries(follower.transcript(), printed);
            if follower.transcript().status != before {
                println!("[{}]", follower.transcript().status.label());
            }
        }
    }
    Ok(())
}

fn print_entries(transcript: &Transcript, from: usize) -> usize {
    for entry in &transcript.entries[from..] {
        println!("{}", entry_line(entry));
    }
    transcript.entries.len()
}

fn entry_line(entry: &Entry) -> String {
    let (label, text) = match (entry.kind, urgent_text(&entry.text)) {
        (EntryKind::User, Some(text)) => ("user (urgent)", text),
        (kind, _) => (kind.label(), entry.text.as_str()),
    };
    format!("{label}: {}", text.trim_end().replace('\n', "\n  "))
}

#[cfg(test)]
#[path = "fleet_tests.rs"]
mod tests;
