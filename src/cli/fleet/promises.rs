use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::time::Duration;

use north_dispatch::laptop::Zone;

use crate::fleet::model::Delivery;
use crate::fleet::promises::{Book, Promise, When, landed_as};
use crate::fleet::registry::{Fleet, root_here};
use crate::fleet::runtime::PROXY;
use crate::fleet::sender::Sender;
use crate::platform::process::run_command_within;

pub(super) const RUN: Duration = Duration::from_secs(60);

pub(super) fn watch(fleet: &Fleet, now_ms: u64) -> Vec<String> {
    let Ok(root) = root_here() else { return Vec::new() };
    let book = Book::at(&root);
    let open = book.open();
    if open.is_empty() {
        return Vec::new();
    }
    let zone = Zone::laptop();
    let (settled, due): (Vec<Promise>, Vec<Promise>) = open.into_iter().filter(|promise| due_ms(promise, &zone).is_none_or(|due| due <= now_ms)).partition(|promise| due_ms(promise, &zone).is_none());
    let keys: Vec<String> = settled.iter().chain(&due).map(Promise::key).collect();
    if let Err(error) = book.checked(&keys) {
        return vec![format!("{} due promises were left for the next tick: their check could not be recorded, so running them now could run them twice: {}", due.len(), error.user_message())];
    }
    due.iter().map(|promise| check(promise, &|text| fleet.send_message(&Sender::fleet(), PROXY, text, Delivery::Now).map(drop).map_err(|error| error.user_message()))).collect()
}

pub(super) fn due_ms(promise: &Promise, zone: &Zone) -> Option<u64> {
    let due = match promise.when {
        When::Utc { at_ms } => at_ms,
        When::Wall { hour, minute } => u64::try_from(zone.nearest(i64::try_from(promise.said_ms / 1000).ok()?, hour.into(), minute.into())?).ok()? * 1000,
    };
    (due > promise.said_ms).then_some(due)
}

pub(super) fn check(promise: &Promise, tell_proxy: &dyn Fn(&str) -> Result<(), String>) -> String {
    let cwd = if promise.cwd.is_dir() { promise.cwd.as_path() } else { Path::new("/") };
    let mut command = Command::new("sh");
    let events = fs::read_to_string(crate::platform::records_mirror::state_home().join("safe-push/events.jsonl")).unwrap_or_default();
    command.args(["-c", &landed_as(&promise.command, &events)]).current_dir(cwd);
    let failure = match run_command_within(command, None, RUN) {
        Some(out) if out.status.success() => return format!("promise kept: {} said {:?}, and `{}` passed", promise.session, promise.line, promise.command),
        Some(out) if misused(&out) => return format!("promise not judged: {} said {:?}, and `{}` failed on its own usage{}, so it reached no verdict", promise.session, promise.line, promise.command, last_line(&out)),
        Some(out) => format!("exited {}{}", out.status.code().map_or_else(|| "on a signal".to_owned(), |code| code.to_string()), last_line(&out)),
        None => format!("did not finish within {} s", RUN.as_secs()),
    };
    let text = format!("promise not kept: {} reported {:?}, and when that time came `{}` {failure}", promise.session, promise.line, promise.command);
    match tell_proxy(&text) {
        Ok(()) => format!("{text} (proxy told)"),
        Err(why) => format!("{text} (not told to the proxy: {why})"),
    }
}

fn misused(out: &Output) -> bool {
    out.status.code() == Some(2)
        || String::from_utf8_lossy(&out.stderr).lines().map(|line| line.trim().to_ascii_lowercase()).any(|line| {
            let message = line.strip_prefix("error:").map_or(line.as_str(), str::trim_start);
            message.starts_with("usage:") || message.starts_with("unrecognized")
        })
}

fn last_line(out: &Output) -> String {
    [&out.stderr, &out.stdout]
        .into_iter()
        .find_map(|bytes| String::from_utf8_lossy(bytes).lines().rev().map(str::trim).find(|line| !line.is_empty()).map(str::to_owned))
        .map(|line| format!(": {line}"))
        .unwrap_or_default()
}

#[cfg(test)]
#[path = "promises_tests.rs"]
mod tests;
