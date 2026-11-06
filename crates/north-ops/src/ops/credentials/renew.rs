use std::path::Path;

use serde_json::{Value, json};

use super::DAY;
use super::model::{Credential, Kind, Renewal, day};
use super::registry::failing;

pub const RECORD: &str = "north/credentials/check.json";
const NOTICE_REPEAT_DAYS: u64 = 7;

pub trait Effects {
    fn run(&self, command: &[String]) -> Result<(), String>;
    fn alert(&self, text: &str) -> Result<(), String>;
    fn dry_run(&self) -> bool;
}

pub struct Outcome {
    pub lines: Vec<String>,
    pub failed: bool,
}

pub struct Plan<'a> {
    pub renew: Vec<(&'a Credential, &'a [String])>,
    pub ask: Vec<&'a Credential>,
}

fn due(credential: &Credential, now: u64, window_days: u64) -> bool {
    credential.expires.is_some_and(|expires| expires <= now + window_days * DAY)
}

pub fn plan(credentials: &[Credential], now: u64, window_days: u64) -> Plan<'_> {
    let mut found = Plan { renew: Vec::new(), ask: Vec::new() };
    for credential in credentials.iter().filter(|credential| due(credential, now, window_days)) {
        match (&credential.kind, &credential.renewal) {
            (Kind::Machine, Renewal::Command(command)) => found.renew.push((credential, command)),
            (_, Renewal::Tom(_)) => found.ask.push(credential),
            _ => {}
        }
    }
    found
}

pub fn notice(ask: &[&Credential], window_days: u64) -> Option<String> {
    if ask.is_empty() {
        return None;
    }
    let listed: Vec<String> = ask.iter().map(|credential| format!("{} ({})", credential.name, credential.expires.map_or_else(|| "?".into(), day))).collect();
    Some(format!("Credentials that need a person within {window_days} days: {}. `north credentials check` names the renewal path of each.", listed.join(", ")))
}

fn previous(record: &Path) -> (Option<String>, u64) {
    let value: Value = std::fs::read_to_string(record).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null);
    (value["notice"].as_str().map(str::to_owned), value["notice_at"].as_u64().unwrap_or(0))
}

fn write(record: &Path, value: &Value) -> Result<(), String> {
    let directory = record.parent().ok_or("record has no directory")?;
    std::fs::create_dir_all(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    let scratch = record.with_extension("json.tmp");
    std::fs::write(&scratch, format!("{value}\n")).and_then(|()| std::fs::rename(&scratch, record)).map_err(|error| format!("{}: {error}", record.display()))
}

pub fn tick(credentials: &[Credential], host: &str, now: u64, window_days: u64, record: &Path, effects: &dyn Effects) -> Outcome {
    let found = plan(credentials, now, window_days);
    let mut lines = Vec::new();
    let mut failed = false;
    let mut renewed = Vec::new();
    let mut unrenewed: Vec<&Credential> = Vec::new();
    for (credential, command) in &found.renew {
        if effects.dry_run() {
            lines.push(format!("would renew {}: {}", credential.name, command.join(" ")));
            continue;
        }
        match effects.run(command) {
            Ok(()) => {
                lines.push(format!("renewed {}", credential.name));
                renewed.push(credential.name.clone());
            }
            Err(error) => {
                lines.push(format!("renewal of {} failed: {error}", credential.name));
                unrenewed.push(credential);
                failed = true;
            }
        }
    }
    let mut ask = found.ask;
    ask.extend(unrenewed);
    let text = notice(&ask, window_days);
    let (sent, sent_at) = previous(record);
    let repeat = sent.as_deref() != text.as_deref() || now.saturating_sub(sent_at) >= NOTICE_REPEAT_DAYS * DAY;
    let mut recorded = (text.as_ref().and(sent.clone()), sent_at);
    if let Some(text) = text.as_deref().filter(|_| repeat) {
        if effects.dry_run() {
            lines.push(format!("would alert: {text}"));
        } else {
            match effects.alert(text) {
                Ok(()) => {
                    lines.push(format!("alerted: {text}"));
                    recorded = (Some(text.to_owned()), now);
                }
                Err(error) => {
                    lines.push(format!("alert failed: {error}; the line was: {text}"));
                    failed = true;
                }
            }
        }
    }
    let hold = failing(credentials, now);
    lines.push(format!("{} credentials, {} failing, {} renewed, {} need a person within {window_days} days", credentials.len(), hold.len(), renewed.len(), ask.len()));
    if !effects.dry_run() {
        let value = json!({
            "at": now,
            "host": host,
            "credentials": credentials.len(),
            "failing": hold.iter().map(|(credential, why)| format!("{}: {why}", credential.name)).collect::<Vec<_>>(),
            "renewed": renewed,
            "notice": recorded.0,
            "notice_at": recorded.1,
        });
        if let Err(error) = write(record, &value) {
            lines.push(format!("record not written: {error}"));
            failed = true;
        }
    }
    Outcome { lines, failed }
}
