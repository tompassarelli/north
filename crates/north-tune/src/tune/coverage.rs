//! Run field coverage: the share of the last day's finished runs that recorded each field the
//! estimate report, routing and the skill-effect table read. A field under the floor fails the tick,
//! because every learned decision would run on part of the data.
use std::time::Duration;

use serde_json::Value;

use crate::ledger;
use crate::platform::sqlite::{self, Access};

pub const FLOOR: f64 = 0.98;
pub const WINDOW: &str = "24h";
const COLUMNS: &str = "role, model, effort, cost_usd, tokens, category, spec, scope, surface, verify, minutes, eta_min, eta_source, budget_usd, budget_source, active_min, turns, tool_calls, brief_text, compactions";

/// What a run must carry for a field to count as recorded.
enum Presence {
    Any(&'static [&'static str]),
    Known(&'static str),
}

const FIELDS: [(&str, Presence, Scope); 16] = [
    ("model", Presence::Any(&["model"]), Scope::Every),
    ("effort", Presence::Any(&["effort"]), Scope::Every),
    ("cost", Presence::Any(&["cost_usd"]), Scope::Every),
    ("tokens", Presence::Any(&["tokens"]), Scope::Every),
    ("category", Presence::Known("category"), Scope::Launched),
    ("facets", Presence::Any(&["spec", "scope", "surface", "verify"]), Scope::Launched),
    ("wall minutes", Presence::Any(&["minutes"]), Scope::Every),
    ("ETA", Presence::Any(&["eta_min"]), Scope::Launched),
    ("ETA source", Presence::Any(&["eta_source"]), Scope::Launched),
    ("E", Presence::Any(&["budget_usd"]), Scope::Launched),
    ("E source", Presence::Any(&["budget_source"]), Scope::Launched),
    ("active minutes", Presence::Any(&["active_min"]), Scope::Every),
    ("turns", Presence::Any(&["turns"]), Scope::Every),
    ("tool calls", Presence::Any(&["tool_calls"]), Scope::Every),
    ("brief text", Presence::Any(&["brief_text"]), Scope::Every),
    ("compactions", Presence::Any(&["compactions"]), Scope::Every),
];

/// Which runs a field is read over: a reviewer, judge, probe or proxy is launched without a category, facets or
/// launch estimates of its own, so those fields are read over the runs that carry a launch brief.
#[derive(Clone, Copy)]
enum Scope {
    Every,
    Launched,
}

const UNLAUNCHED_ROLES: [&str; 4] = ["reviewer", "judge", "probe", "proxy"];

impl Scope {
    fn covers(self, run: &Value) -> bool {
        match self {
            Self::Every => true,
            Self::Launched => !run["role"].as_str().is_some_and(|role| UNLAUNCHED_ROLES.contains(&role)),
        }
    }
}

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
        if self.runs == 0 { 1.0 } else { self.recorded as f64 / self.runs as f64 }
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
    FIELDS
        .iter()
        .map(|(field, presence, scope)| {
            let covered: Vec<&Value> = runs.iter().filter(|run| scope.covers(run)).collect();
            Share { field, recorded: covered.iter().filter(|run| presence.holds(run)).count(), runs: covered.len() }
        })
        .collect()
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
            let set_aside = runs.iter().filter(|run| !Scope::Launched.covers(run)).count();
            if set_aside > 0 {
                lines.push(format!("  category, facets and ETA read over launched work: {set_aside} reviewer, judge, probe or proxy runs set aside"));
            }
            (lines, !short.is_empty())
        }
    }
}
