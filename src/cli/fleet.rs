use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Delivery, EntryKind, LaunchRequest, ProviderName, SessionRecord, Status, Transcript};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::{Runtimes, new_uuid};
use crate::providers::claude::fleet::Claude;
use crate::providers::codex::fleet::Codex;
use crate::tui;

const USAGE: &str = "Usage: north fleet [list [--json] | tree | accounts | launch --provider claude|codex --account A [--cwd D] [--model M] [--name N] [--system-prompt FILE] [--foreground-cap SECS] [--idle-exit MINUTES] [--parent NAME] PROMPT... \
     | send [--now] NAME TEXT... | ask NAME QUESTION... [--timeout SECS] | interrupt NAME | stop NAME | forget NAME | revive NAME [--system-prompt FILE] | show NAME [--follow | --json]]";
const DEFAULT_ASK_SECS: u64 = 120;
const FOLLOW_POLL: Duration = Duration::from_millis(250);

pub fn run(arguments: &[String]) -> ExitCode {
    let result = Fleet::open(runtimes()).and_then(|fleet| dispatch(fleet, arguments));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {}", error.user_message());
            ExitCode::FAILURE
        }
    }
}

fn runtimes() -> Runtimes {
    Runtimes::new(vec![Arc::new(Claude), Arc::new(Codex)])
}

fn dispatch(fleet: Fleet, arguments: &[String]) -> NorthResult<()> {
    let words: Vec<&str> = arguments.iter().map(String::as_str).collect();
    match words.as_slice() {
        [] => tui::fleet::run(fleet),
        ["list", rest @ ..] => list(&fleet, rest),
        ["tree"] => tree(&fleet),
        ["accounts"] => accounts(&fleet),
        ["launch", rest @ ..] => launch(&fleet, rest),
        ["send", rest @ ..] => send(&fleet, rest),
        ["ask", name, rest @ ..] => ask(&fleet, name, rest),
        [verb, name] => session_verb(&fleet, verb, name),
        ["revive", name, "--system-prompt", file] => revive(&fleet, name, Some(absolute(file)?)),
        ["show", name, flag] => show_with(&fleet, name, flag),
        _ => Err(usage()),
    }
}

fn session_verb(fleet: &Fleet, verb: &str, name: &str) -> NorthResult<()> {
    match verb {
        "interrupt" => fleet.interrupt(name),
        "stop" => fleet.stop(name),
        "forget" => fleet.forget(name),
        "revive" => revive(fleet, name, None),
        "show" => show(fleet, name, false),
        _ => Err(usage()),
    }
}

fn send(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    match words {
        ["--now", name, text @ ..] if !text.is_empty() => fleet.send(name, &text.join(" "), Delivery::Now),
        [name, text @ ..] if !text.is_empty() => fleet.send(name, &text.join(" "), Delivery::Queued),
        _ => Err(usage()),
    }
}

fn revive(fleet: &Fleet, name: &str, system_prompt: Option<PathBuf>) -> NorthResult<()> {
    fleet.revive(name, system_prompt).map(|record| println!("{}", record.name))
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
        let status = fleet.status(&record);
        println!("{}\t{}\t{}\t{}\t{}", record.name, record.provider.label(), record.account, status.label(), record.cwd.display());
    }
    Ok(())
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
        "created_ms": record.created_ms,
        "host_pid": record.host_pid,
    })
}

fn show_json(fleet: &Fleet, name: &str) -> NorthResult<()> {
    let follower = fleet.follow(name)?;
    let transcript = follower.transcript();
    let entries: Vec<Value> = transcript.entries.iter().map(|entry| json!({"kind": kind_label(entry.kind), "text": entry.text})).collect();
    let mut session = session_json(follower.record(), transcript.status);
    session["active_turn"] = json!(transcript.active_turn);
    session["entries"] = Value::Array(entries);
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
    for account in fleet.accounts() {
        println!("{}\t{}", account.provider.label(), account.label);
    }
    Ok(())
}

fn launch(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    let request = launch_request(&fleet.providers(), words)?;
    let record = fleet.launch(&request)?;
    println!("{}", record.name);
    Ok(())
}

#[derive(Default)]
struct LaunchFlags {
    provider: Option<ProviderName>,
    account: Option<String>,
    cwd: Option<PathBuf>,
    model: Option<String>,
    name: Option<String>,
    system_prompt: Option<PathBuf>,
    foreground_cap_secs: Option<u64>,
    idle_exit_minutes: Option<u64>,
    parent: Option<String>,
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
            "--foreground-cap" => self.foreground_cap_secs = Some(value.parse().map_err(|_| usage())?),
            "--idle-exit" => self.idle_exit_minutes = Some(value.parse().map_err(|_| usage())?),
            "--parent" => self.parent = Some(value),
            _ => return Err(usage()),
        }
        Ok(())
    }

    fn request(self, prompt: String) -> NorthResult<LaunchRequest> {
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
            name,
            provider,
            account,
            cwd,
            model: self.model,
            prompt,
            system_prompt: self.system_prompt,
            foreground_cap_secs: self.foreground_cap_secs,
            idle_exit_minutes: self.idle_exit_minutes,
            parent: self.parent,
        })
    }
}

fn launch_request(providers: &[ProviderName], words: &[&str]) -> NorthResult<LaunchRequest> {
    let mut flags = LaunchFlags::default();
    let mut index = 0;
    while let Some(flag) = words.get(index).filter(|word| word.starts_with("--")) {
        let value = words.get(index + 1).map(|value| (*value).to_owned()).ok_or_else(usage)?;
        flags.set(providers, flag, value)?;
        index += 2;
    }
    flags.request(words[index..].join(" "))
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
        println!("{}: {}", kind_label(entry.kind), entry.text);
    }
    transcript.entries.len()
}

fn kind_label(kind: EntryKind) -> &'static str {
    match kind {
        EntryKind::User => "user",
        EntryKind::Assistant => "assistant",
        EntryKind::Tool => "tool",
        EntryKind::ToolResult => "result",
        EntryKind::TurnEnd => "turn",
        EntryKind::Error => "error",
        EntryKind::Notice => "notice",
    }
}
