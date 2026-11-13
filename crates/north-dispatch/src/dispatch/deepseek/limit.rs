//! The Go plan's usage limit as dispatch sees it: the fleet parks an opencode session whose turn ended on the limit on a
//! `usage:` wait, so its listing row names the time the limit lifts. Dispatch records that time, places no opencode box
//! before it under spend's provider-limit refusal, and forgets each parked session so its box, already tried, goes to Claude.

use super::{ACCOUNT, PROVIDER};
use crate::foundation::clock::rfc3339_seconds;
use crate::spend::{Account, Refusal, Starter, View, Window};
use serde_json::{Value, json};
use std::path::Path;

const RECORD: &str = "deepseek-limit.json";
const FIVE_HOURS_S: f64 = 5.0 * 3600.0;

/// Runs `north WORDS` on a host, as `north fleet forget` there.
pub type Forget<'a> = dyn FnMut(&str, &[String]) -> Result<(), String> + 'a;

/// An opencode session the limit stopped: its host, its name and when its wait lifts.
#[derive(Clone, Debug, PartialEq)]
pub struct Stopped {
    pub host: String,
    pub name: String,
    pub until: f64,
}

/// What dispatch knows of the Go limit at one moment: when it lifts, if it holds now, and the sessions it stopped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Limit {
    pub until: Option<f64>,
    pub stopped: Vec<Stopped>,
}

/// When `session`'s `usage:` wait lifts, for an opencode session the fleet parked on its limit.
fn parked_until(session: &Value) -> Option<f64> {
    if session["provider"] != PROVIDER || session["past"] == true {
        return None;
    }
    let waits = session["waiting"].as_array()?.iter().filter_map(Value::as_str);
    waits.filter_map(|run| run.strip_prefix("usage:")?.split_once('@').and_then(|(_, time)| rfc3339_seconds(time))).reduce(f64::max)
}

/// Whether `session` stopped on its provider's limit, so it holds no slot of the provider's cap.
pub fn parked(session: &Value) -> bool {
    parked_until(session).is_some()
}

impl Limit {
    /// The limit at `at` from every host's listing and the time an earlier run recorded in `directory`.
    pub fn read(lists: &[(String, Value)], directory: &Path, at: f64) -> Self {
        let stopped: Vec<Stopped> = lists
            .iter()
            .flat_map(|(host, listing)| listing.as_array().into_iter().flatten().filter_map(move |session| Some(Stopped { host: host.clone(), name: session["name"].as_str()?.to_owned(), until: parked_until(session)? })))
            .collect();
        let recorded = std::fs::read(directory.join(RECORD)).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()).and_then(|record| record["until"].as_f64());
        let until = stopped.iter().map(|session| session.until).chain(recorded).reduce(f64::max).filter(|until| *until > at);
        Self { until, stopped }
    }

    /// Spend's provider-limit refusal of `opencode:go` at `at` while the limit holds.
    pub fn refusal(&self, at: f64) -> Option<Refusal> {
        let until = self.until?;
        let full = Some(Window { used_pct: 100.0, resets_at: Some(until) });
        let week = until - at > FIVE_HOURS_S;
        let account = Account { name: ACCOUNT.into(), plan_week: 0.0, five_hour: if week { None } else { full }, week: if week { full } else { None }, counted: false };
        View { at, accounts: vec![account], ..View::default() }.admit(ACCOUNT, Starter::Worker).err()
    }

    /// Records when the limit lifts in `directory` and forgets each session it stopped through `forget`, one line each.
    pub fn release(&self, directory: &Path, forget: &mut Forget<'_>, dry_run: bool) -> Vec<String> {
        let would = if dry_run { "would " } else { "" };
        let mut lines = Vec::new();
        if let Some(until) = self.until.filter(|_| !dry_run) {
            let written = std::fs::create_dir_all(directory).and_then(|()| std::fs::write(directory.join(RECORD), json!({"until": until}).to_string()));
            if let Err(why) = written {
                lines.push(format!("the Go limit until {until} was not recorded: {why}"));
            }
        }
        for session in &self.stopped {
            let line = format!("{would}forget {} on {}: it stopped on the Go plan's limit, so its box goes to Claude", session.name, session.host);
            let forgot = if dry_run { Ok(()) } else { forget(&session.host, &["fleet".to_owned(), "forget".to_owned(), session.name.clone()]) };
            lines.push(match forgot {
                Ok(()) => line,
                Err(why) => format!("{line}; not forgotten: {why}"),
            });
        }
        lines
    }
}
