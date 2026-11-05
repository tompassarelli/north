//! Run field coverage: the share of the last day's finished runs that recorded each field the
//! estimate report, routing and the skill-effect table read. A field under the floor fails the tick,
//! because every learned decision would run on part of the data.
use std::time::Duration;

use serde_json::Value;

use crate::ledger;
use crate::platform::sqlite::{self, Access};

pub const FLOOR: f64 = 0.98;
pub const WINDOW: &str = "24h";
const COLUMNS: &str = "model, effort, cost_usd, tokens, category, spec, scope, surface, verify, minutes, eta_min, active_min, turns, tool_calls, brief_text, compactions";

/// What a run must carry for a field to count as recorded.
enum Presence {
    Any(&'static [&'static str]),
    Known(&'static str),
}

const FIELDS: [(&str, Presence); 13] = [
    ("model", Presence::Any(&["model"])),
    ("effort", Presence::Any(&["effort"])),
    ("cost", Presence::Any(&["cost_usd"])),
    ("tokens", Presence::Any(&["tokens"])),
    ("category", Presence::Known("category")),
    ("facets", Presence::Any(&["spec", "scope", "surface", "verify"])),
    ("wall minutes", Presence::Any(&["minutes"])),
    ("ETA", Presence::Any(&["eta_min"])),
    ("active minutes", Presence::Any(&["active_min"])),
    ("turns", Presence::Any(&["turns"])),
    ("tool calls", Presence::Any(&["tool_calls"])),
    ("brief text", Presence::Any(&["brief_text"])),
    ("compactions", Presence::Any(&["compactions"])),
];

fn recorded(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(text) => !text.trim().is_empty(),
        _ => true,
    }
}

impl Presence {
    fn holds(&self, run: &Value) -> bool {
        match self {
            Self::Any(columns) => columns.iter().all(|column| recorded(&run[*column])),
            Self::Known(column) => run[*column].as_str().is_some_and(|text| !text.is_empty() && text != "unknown"),
        }
    }
}

/// One field's share of the window's runs.
#[derive(Debug, PartialEq)]
pub struct Share {
    pub field: &'static str,
    pub recorded: usize,
    pub runs: usize,
}

impl Share {
    fn ratio(&self) -> f64 {
        self.recorded as f64 / self.runs as f64
    }

    fn short(&self) -> bool {
        self.ratio() < FLOOR
    }

    fn line(&self) -> String {
        let percent = (self.ratio() * 1000.0).floor() / 10.0;
        let mark = if self.short() { "  BELOW" } else { "" };
        format!("  {} {percent}% ({} of {}){mark}", self.field, self.recorded, self.runs)
    }
}

/// The finished runs of the ledger that ended in the last day.
pub fn read() -> Result<Vec<Value>, String> {
    let since = ledger::window_start(WINDOW).map_err(|error| error.user_message())?;
    let query = format!("SELECT {COLUMNS} FROM runs WHERE outcome != 'running' AND coalesce(ended, '') >= '{}'", since.replace('\'', ""));
    sqlite::rows(&ledger::threads_database(), Access::ReadOnly, Duration::from_secs(5), &query).map_err(|error| error.user_message())
}

pub fn shares(runs: &[Value]) -> Vec<Share> {
    FIELDS.iter().map(|(field, presence)| Share { field, recorded: runs.iter().filter(|run| presence.holds(run)).count(), runs: runs.len() }).collect()
}

/// The coverage table's lines and whether a field fell under the floor or the runs went unread.
pub fn check(runs: &Result<Vec<Value>, String>) -> (Vec<String>, bool) {
    let floor = FLOOR * 100.0;
    match runs {
        Err(error) => (vec![format!("run field coverage {WINDOW}: unread ({error})")], true),
        Ok(runs) if runs.is_empty() => (vec![format!("run field coverage {WINDOW}: no finished runs")], false),
        Ok(runs) => {
            let shares = shares(runs);
            let short: Vec<&str> = shares.iter().filter(|share| share.short()).map(|share| share.field).collect();
            let verdict = if short.is_empty() { "every field holds".to_owned() } else { format!("under {floor}%: {}", short.join(", ")) };
            let mut lines = vec![format!("run field coverage {WINDOW} over {} finished runs, floor {floor}%: {verdict}", runs.len())];
            lines.extend(shares.iter().map(Share::line));
            (lines, !short.is_empty())
        }
    }
}
