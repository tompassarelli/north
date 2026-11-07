use std::fs::{self, OpenOptions};
use std::io::Write;

use super::super::deps;
use super::super::select::Config;
use super::arguments::{Arguments, Outcome};
use super::fail_fast;
use super::{say, toplevel, warn};
use crate::landing::check_timings::outcomes::{Record, Records};

pub(super) fn portable(arguments: &Arguments) -> u8 {
    let Some(root) = toplevel() else { return 1 };
    let config = Config::load(&root, &arguments.head);
    let Ok(commands) = config.commands(&root, &arguments.base, &arguments.head) else { return 1 };
    let portable = config.workflow.is_empty() && !commands.is_empty() && commands.iter().all(|command| config.local_reason(command).is_none());
    u8::from(!portable)
}

fn record_wait(arguments: &Arguments) {
    let Some(log) = &arguments.event_log else { return };
    if let Some(directory) = log.parent() {
        let _ = fs::create_dir_all(directory);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log) {
        let _ = writeln!(file, "{}", arguments.wait_event);
    }
}

pub(super) fn start_stage(arguments: &Arguments) -> Option<u8> {
    let destination = arguments.destination.as_deref()?;
    if destination != "main" {
        return Some(0);
    }
    let Some(root) = toplevel() else { return Some(0) };
    if !Config::load(&root, "HEAD").has_checks() {
        return Some(0);
    }
    record_wait(arguments);
    let Err(error) = deps::sync(&root) else { return None };
    warn(&error);
    Outcome(arguments.outcome.clone()).record(&["skip", "skipped: bun install failed"]);
    Some(1)
}

pub(super) fn any(arguments: &Arguments) -> u8 {
    let Some(root) = toplevel() else { return 1 };
    u8::from(!Config::load(&root, &arguments.head).has_checks())
}

pub(super) fn replay(arguments: &Arguments) -> u8 {
    let until = if arguments.until == 0 { u64::MAX } else { arguments.until };
    let records: Vec<Record> = Records::at(arguments.records.clone().unwrap_or_else(Records::default_path)).read().into_iter().filter(|record| (arguments.since..until).contains(&record.started)).collect();
    say(&fail_fast::replay(&records).to_string());
    0
}
