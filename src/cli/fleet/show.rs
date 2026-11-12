use std::io::Write;
use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Entry, EntryKind, Status, Transcript};
use crate::fleet::registry::Fleet;
use crate::fleet::sender::urgent_text;

use super::list;
use super::turn_json::turn_json;
use super::usage::usage;

const FOLLOW_POLL: Duration = Duration::from_millis(250);

pub(super) fn run(fleet: &Fleet, name: &str, flag: &str) -> NorthResult<()> {
    match flag {
        "--follow" => show(fleet, name, true),
        "--json" => show_json(fleet, name),
        _ => Err(usage()),
    }
}

fn show_json(fleet: &Fleet, name: &str) -> NorthResult<()> {
    let follower = match fleet.follow(name) {
        Err(NorthError::Usage(_)) => fleet.follow_retired(name)?,
        followed => followed?,
    };
    let transcript = follower.transcript();
    let entries: Vec<Value> = transcript.entries.iter().map(|entry| json!({"kind": entry.kind.label(), "text": entry.text})).collect();
    let mut session = list::session_json(follower.record(), transcript.status, &fleet.waits());
    session["active_turn"] = json!(transcript.active_turn);
    session["entries"] = Value::Array(entries);
    session["turns"] = transcript.turns.iter().map(turn_json).collect();
    println!("{session}");
    Ok(())
}

fn queued_line(fleet: &Fleet, name: &str) -> Option<String> {
    let queued = fleet.queued_launches().ok()?.into_iter().find(|queued| queued.name == name)?;
    Some(format!("[queued] {} is queued at {} priority: {}", queued.name, queued.priority.label(), queued.refused))
}

pub(super) fn show(fleet: &Fleet, name: &str, follow: bool) -> NorthResult<()> {
    show_to(fleet, name, follow, &mut std::io::stdout().lock())
}

fn show_to(fleet: &Fleet, name: &str, follow: bool, out: &mut dyn Write) -> NorthResult<()> {
    let mut follower = match fleet.follow(name) {
        Err(error @ NorthError::Usage(_)) => match fleet.follow_retired(name) {
            Ok(retired) => retired,
            Err(NorthError::Usage(_)) => match queued_line(fleet, name) {
                Some(line) => {
                    writeln!(out, "{line}")?;
                    return Ok(());
                }
                None => return Err(error),
            },
            Err(other) => return Err(other),
        },
        followed => followed?,
    };
    let mut printed = print_entries(out, follower.transcript(), 0)?;
    writeln!(out, "[{}]", follower.transcript().status.label())?;
    while follow && follower.transcript().status != Status::Exited {
        std::thread::sleep(FOLLOW_POLL);
        let before = follower.transcript().status;
        if follower.poll()? {
            printed = print_entries(out, follower.transcript(), printed)?;
            if follower.transcript().status != before {
                writeln!(out, "[{}]", follower.transcript().status.label())?;
            }
        }
    }
    Ok(())
}

fn print_entries(out: &mut dyn Write, transcript: &Transcript, from: usize) -> NorthResult<usize> {
    for entry in &transcript.entries[from..] {
        writeln!(out, "{}", entry_line(entry))?;
    }
    Ok(transcript.entries.len())
}

pub(super) fn entry_line(entry: &Entry) -> String {
    let (label, text) = match (entry.kind, urgent_text(&entry.text)) {
        (EntryKind::User, Some(text)) => ("user (urgent)", text),
        (kind, _) => (kind.label(), entry.text.as_str()),
    };
    format!("{label}: {}", text.trim_end().replace('\n', "\n  "))
}

#[cfg(test)]
#[path = "show_tests.rs"]
mod tests;
