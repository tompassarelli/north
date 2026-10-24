use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Delivery, Entry, EntryKind, LaunchRequest, ProviderName, SessionRecord, Status, Transcript};
use crate::fleet::prompt::PromptMode;
use crate::fleet::registry::{self, Fleet, urgent_text};
use crate::fleet::roster::Roster;
use crate::fleet::runtime::{Runtimes, SESSION_ENV, new_uuid};
use crate::providers::claude::fleet::Claude;
use crate::providers::codex::fleet::Codex;
use crate::fleet::turns::Turn;
use crate::ledger::Ledger;
use crate::session_host::{FrameWatch, HostConfig};
use crate::tui;

use super::fleet_placement::{self, Launch, Overrides, Placed};
use super::{gate, orders, relay};

const USAGE: &str = "Usage: north fleet [list [--json] | tree | accounts | launch --provider claude|codex --account A [--cwd D] [--model M] [--name N] [--system-prompt FILE] [--prompt north|vanilla] [--foreground-cap SECS] [--idle-exit MINUTES] [--parent NAME|none] [--task REF] [--local REASON] [--pin REASON] [--test] PROMPT... \
     | send [--now] NAME TEXT... | relay HOST... | outbox take --for HOST | outbox drop ID... | ask NAME QUESTION... [--timeout SECS] | interrupt NAME | stop NAME | forget NAME | revive NAME [--system-prompt FILE] [--local REASON] [--pin REASON] | show NAME [--follow | --json]]";
const DEFAULT_ASK_SECS: u64 = 120;
const FOLLOW_POLL: Duration = Duration::from_millis(250);

pub fn run(arguments: &[String]) -> ExitCode {
    let result = open_fleet().and_then(|fleet| {
        default_overdue_gates(&fleet);
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

fn open_fleet() -> NorthResult<Fleet> {
    Fleet::open(runtimes(), Arc::new(Ledger::open_default()?))
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
        ["list", rest @ ..] => list(&fleet, rest),
        ["tree"] => tree(&fleet),
        ["accounts"] => accounts(&fleet),
        ["launch", rest @ ..] => launch(&fleet, rest),
        [verb @ ("send" | "relay" | "outbox"), rest @ ..] => relay::run(&fleet, verb, rest),
        ["ask", name, rest @ ..] => ask(&fleet, name, rest),
        ["orders", rest @ ..] => orders::run(&fleet, rest),
        ["gate", rest @ ..] => gate::run(&fleet, rest),
        [verb, name] => session_verb(&fleet, verb, name),
        ["revive", name, rest @ ..] => revive_with(&fleet, name, rest),
        ["show", name, flag] => show_with(&fleet, name, flag),
        _ => Err(usage()),
    }
}

fn session_verb(fleet: &Fleet, verb: &str, name: &str) -> NorthResult<()> {
    match verb {
        "interrupt" => fleet.interrupt(name),
        "stop" => fleet.stop(name),
        "forget" => fleet.forget(name),
        "revive" => revive(fleet, name, None, Overrides::default()),
        "show" => show(fleet, name, false),
        "watchdog" => fleet.guard(name),
        _ => Err(usage()),
    }
}

fn revive_with(fleet: &Fleet, name: &str, words: &[&str]) -> NorthResult<()> {
    let (mut system_prompt, mut overrides) = (None, Overrides::default());
    for pair in words.chunks(2) {
        match pair {
            ["--system-prompt", file] => system_prompt = Some(absolute(file)?),
            [flag, value] if overrides.set(flag, value) => {}
            _ => return Err(usage()),
        }
    }
    revive(fleet, name, system_prompt, overrides)
}

/// Revive re-runs placement for a stopped Claude session, keeping the reasons its launch recorded and
/// the machine its conversation lives on.
fn revive(fleet: &Fleet, name: &str, system_prompt: Option<PathBuf>, overrides: Overrides) -> NorthResult<()> {
    let record = fleet.session(name)?;
    if let (Some(machine), Some(_)) = (&record.machine, &overrides.local) {
        return Err(NorthError::Usage(format!("{name}'s conversation lives on {}, so it revives there; launch a new session to work here", machine.name)));
    }
    let mut placement = None;
    if record.provider.label() == "claude" && !record.verification && !fleet.running(&record) {
        let overrides = overrides.recorded(record.placement.as_deref());
        let brief = system_prompt.clone().or_else(|| record.system_prompt.clone());
        let launch = Launch { name, account: &record.account, brief: brief.as_deref(), cwd: None };
        placement = fleet_placement::check(&launch, &fleet_placement::brief_text(brief.as_deref(), "")?, &overrides, &machine_load(fleet)?, &fleet.roster())?.note;
    }
    fleet.revive_placed(name, system_prompt, placement).map(|record| println!("{}", record.name))
}

fn show_with(fleet: &Fleet, name: &str, flag: &str) -> NorthResult<()> {
    match flag {
        "--follow" => show(fleet, name, true),
        "--json" => show_json(fleet, name),
        _ => Err(usage()),
    }
}

fn usage() -> NorthError {
    NorthError::Usage(USAGE.into())
}

fn list(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    match words {
        [] => list_lines(fleet),
        ["--json"] => list_json(fleet),
        _ => Err(usage()),
    }
}

fn list_lines(fleet: &Fleet) -> NorthResult<()> {
    for record in fleet.sessions()? {
        println!("{}", list_line(&record, fleet.status(&record)));
    }
    Ok(())
}

fn list_line(record: &SessionRecord, status: Status) -> String {
    let cwd = match &record.machine {
        Some(machine) => format!("{}:{}", machine.name, record.cwd.display()),
        None => record.cwd.display().to_string(),
    };
    let line = format!("{}\t{}\t{}\t{}\t{cwd}", record.name, record.provider.label(), record.account, status.label());
    match &record.placement {
        Some(note) => format!("{line}\t{note}"),
        None => line,
    }
}

fn list_json(fleet: &Fleet) -> NorthResult<()> {
    let sessions: Vec<Value> = fleet.sessions()?.iter().map(|record| session_json(record, fleet.status(record))).collect();
    println!("{}", Value::Array(sessions));
    Ok(())
}

fn session_json(record: &SessionRecord, status: Status) -> Value {
    json!({
        "name": record.name,
        "provider": record.provider.label(),
        "account": record.account,
        "status": status.label(),
        "parent": record.parent,
        "cwd": record.cwd,
        "model": record.model,
        "conversation": record.conversation,
        "task_ref": record.task_ref,
        "created_ms": record.created_ms,
        "host_pid": record.host_pid,
        "placement": record.placement,
        "machine": record.machine.as_ref().map(|machine| &machine.name),
    })
}

fn show_json(fleet: &Fleet, name: &str) -> NorthResult<()> {
    let follower = fleet.follow(name)?;
    let transcript = follower.transcript();
    let entries: Vec<Value> = transcript.entries.iter().map(|entry| json!({"kind": entry.kind.label(), "text": entry.text})).collect();
    let mut session = session_json(follower.record(), transcript.status);
    session["active_turn"] = json!(transcript.active_turn);
    session["entries"] = Value::Array(entries);
    session["turns"] = transcript.turns.iter().map(turn_json).collect();
    println!("{session}");
    Ok(())
}

fn tree(fleet: &Fleet) -> NorthResult<()> {
    let sessions = fleet.sessions()?;
    let known = |name: &String| sessions.iter().any(|record| &record.name == name);
    for root in sessions.iter().filter(|record| !record.parent.as_ref().is_some_and(known)) {
        print_branch(fleet, &sessions, root, 0);
    }
    Ok(())
}

fn print_branch(fleet: &Fleet, sessions: &[SessionRecord], record: &SessionRecord, depth: usize) {
    let status = fleet.status(record);
    println!("{}{} [{}] {}/{}", "  ".repeat(depth), record.name, status.label(), record.provider.label(), record.account);
    for child in sessions.iter().filter(|child| child.parent.as_deref() == Some(record.name.as_str())) {
        print_branch(fleet, sessions, child, depth + 1);
    }
}

fn accounts(fleet: &Fleet) -> NorthResult<()> {
    let roster = fleet.roster();
    for account in &roster.accounts {
        println!("{}\t{}", account.provider.label(), account.label);
    }
    for duplicate in &roster.duplicates {
        eprintln!("omitted: {duplicate}");
    }
    Ok(())
}

fn launch(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let (mut request, overrides) = launch_parts(&fleet.providers(), words, std::env::var(SESSION_ENV).ok())?;
    let placed = placed(fleet, &request, &overrides)?;
    (request.placement, request.machine) = (placed.note, placed.machine);
    let record = fleet.launch(&request)?;
    println!("{}", record.name);
    Ok(())
}

/// Placement gates every Claude launch except `--test` sessions, before anything starts.
fn placed(fleet: &Fleet, request: &LaunchRequest, overrides: &Overrides) -> NorthResult<Placed> {
    if request.provider.label() != "claude" || request.verification {
        return Ok(Placed::default());
    }
    let text = fleet_placement::brief_text(request.system_prompt.as_deref(), &request.prompt)?;
    let launch = Launch { name: &request.name, account: &request.account, brief: request.system_prompt.as_deref(), cwd: Some(&request.cwd) };
    fleet_placement::check(&launch, &text, overrides, &machine_load(fleet)?, &fleet.roster())
}

/// Live fleet sessions per machine they run on, which those machines cannot count themselves.
fn machine_load(fleet: &Fleet) -> NorthResult<BTreeMap<String, i64>> {
    let mut load = BTreeMap::new();
    for record in fleet.sessions()? {
        if let Some(machine) = record.machine.as_ref().filter(|_| fleet.running(&record)) {
            *load.entry(machine.name.clone()).or_default() += 1;
        }
    }
    Ok(load)
}

#[derive(Default)]
struct LaunchFlags {
    verification: bool,
    provider: Option<ProviderName>,
    account: Option<String>,
    cwd: Option<PathBuf>,
    model: Option<String>,
    name: Option<String>,
    system_prompt: Option<PathBuf>,
    prompt_mode: PromptMode,
    foreground_cap_secs: Option<u64>,
    idle_exit_minutes: Option<u64>,
    parent: Option<String>,
    task_ref: Option<String>,
    overrides: Overrides,
}

impl LaunchFlags {
    fn set(&mut self, providers: &[ProviderName], flag: &str, value: String) -> NorthResult<()> {
        match flag {
            "--provider" => self.provider = Some(parse_provider(providers, &value)?),
            "--account" => self.account = Some(value),
            "--cwd" => self.cwd = Some(PathBuf::from(value)),
            "--model" => self.model = Some(value),
            "--name" => self.name = Some(value),
            "--system-prompt" => self.system_prompt = Some(absolute(&value)?),
            "--prompt" => self.prompt_mode = PromptMode::parse(&value).ok_or_else(usage)?,
            "--foreground-cap" => self.foreground_cap_secs = Some(value.parse().map_err(|_| usage())?),
            "--idle-exit" => self.idle_exit_minutes = Some(value.parse().map_err(|_| usage())?),
            "--parent" => self.parent = Some(value),
            "--task" => self.task_ref = Some(value),
            _ if self.overrides.set(flag, &value) => {}
            _ => return Err(usage()),
        }
        Ok(())
    }

    fn request(self, prompt: String, caller: Option<String>) -> NorthResult<LaunchRequest> {
        let (Some(provider), Some(account)) = (self.provider, self.account) else { return Err(usage()) };
        if prompt.is_empty() {
            return Err(usage());
        }
        let cwd = match self.cwd {
            Some(cwd) => std::fs::canonicalize(cwd)?,
            None => std::env::current_dir()?,
        };
        let name = match self.name {
            Some(name) => name,
            None => default_name(&provider, &account)?,
        };
        Ok(LaunchRequest {
            verification: self.verification,
            name,
            provider,
            account,
            cwd,
            model: self.model,
            prompt,
            system_prompt: self.system_prompt,
            foreground_cap_secs: self.foreground_cap_secs,
            prompt_mode: self.prompt_mode,
            idle_exit_minutes: self.idle_exit_minutes,
            parent: parent(self.parent, caller),
            task_ref: self.task_ref,
            placement: None,
            machine: None,
        })
    }
}

fn parent(flag: Option<String>, caller: Option<String>) -> Option<String> {
    match flag.as_deref() {
        Some("none") => None,
        Some(_) => flag,
        None => caller.filter(|name| !name.is_empty()),
    }
}

#[cfg(test)]
fn launch_request(providers: &[ProviderName], words: &[&str], caller: Option<String>) -> NorthResult<LaunchRequest> {
    launch_parts(providers, words, caller).map(|(request, _)| request)
}

fn launch_parts(providers: &[ProviderName], words: &[&str], caller: Option<String>) -> NorthResult<(LaunchRequest, Overrides)> {
    let mut flags = LaunchFlags::default();
    let mut index = 0;
    while let Some(flag) = words.get(index).filter(|word| word.starts_with("--")) {
        if *flag == "--test" {
            flags.verification = true;
            index += 1;
            continue;
        }
        let value = words.get(index + 1).map(|value| (*value).to_owned()).ok_or_else(usage)?;
        flags.set(providers, flag, value)?;
        index += 2;
    }
    let overrides = std::mem::take(&mut flags.overrides);
    Ok((flags.request(words[index..].join(" "), caller)?, overrides))
}

fn absolute(path: &str) -> NorthResult<PathBuf> {
    Ok(std::fs::canonicalize(path)?)
}

fn ask(fleet: &Fleet, name: &str, words: &[&str]) -> NorthResult<()> {
    let (question, timeout) = match words {
        [question @ .., "--timeout", secs] => (question, secs.parse().map_err(|_| usage())?),
        question => (question, DEFAULT_ASK_SECS),
    };
    if question.is_empty() {
        return Err(usage());
    }
    match fleet.ask(name, &question.join(" "), Duration::from_secs(timeout))? {
        Some(reply) => {
            println!("{reply}");
            Ok(())
        }
        None => Err(NorthError::Rejected(format!("{name} did not answer within {timeout} s"))),
    }
}

fn parse_provider(providers: &[ProviderName], value: &str) -> NorthResult<ProviderName> {
    providers.iter().find(|provider| provider.label() == value).cloned().ok_or_else(usage)
}

fn default_name(provider: &ProviderName, account: &str) -> NorthResult<String> {
    let short: String = new_uuid()?.chars().take(6).collect();
    Ok(format!("{}-{account}-{short}", provider.label()))
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
