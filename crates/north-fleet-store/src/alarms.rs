use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::sender::Sender;

pub const FILE: &str = "alarms.jsonl";
const PROMISE_PREFIX: &str = "promise not kept:";
const SERVICES: &[(&str, &str)] = &[("north-health", "north-health"), ("north-land-watch", "north-land-watch"), ("north-ops-flow", "ops-flow")];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Alarm {
    pub at_ms: u64,
    pub source: String,
    pub text: String,
}

fn is_notice_of_clearing(text: &str) -> bool {
    text.ends_with(": recovered") || text.starts_with("north ops watch: cleared after")
}

/// The alarm source a message came from: north-health, north-land-watch and north-ops-flow by their service name, and the fleet's due-promise check by its text. Anything else, and a notice that an alarm cleared, is none.
pub fn source_of(sender: &Sender, text: &str) -> Option<&'static str> {
    let Sender::Service(unit) = sender else { return None };
    let unit = unit.split('@').next().unwrap_or(unit);
    if unit == "north-fleet" {
        return text.starts_with(PROMISE_PREFIX).then_some("promise-checker");
    }
    SERVICES.iter().find(|(service, _)| *service == unit).map(|(_, source)| *source).filter(|_| !is_notice_of_clearing(text))
}

/// Append the message to `root`'s alarm log when it is an alarm; any other message leaves the log alone.
pub fn record(root: &Path, sender: &Sender, text: &str, at_ms: u64) {
    let Some(source) = source_of(sender, text) else { return };
    let row = Alarm { at_ms, source: source.to_owned(), text: text.to_owned() };
    let Ok(mut line) = serde_json::to_string(&row) else { return };
    line.push('\n');
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(root.join(FILE)) {
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn read(root: &Path) -> Vec<Alarm> {
    fs::read_to_string(root.join(FILE)).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}
