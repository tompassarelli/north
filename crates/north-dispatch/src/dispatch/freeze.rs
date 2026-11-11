//! The launch freeze alarm: while dispatch runs find ready boxes and free slots and launch none for
//! `dispatch_freeze_alarm_min` minutes, the proxy gets one urgent message naming the guards that refuse.
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::Outcome;
use super::record;
use super::survey::Survey;
use crate::platform::process::run_within;
use crate::spend::Starter;

const FILE: &str = "freeze.json";

/// One stretch of frozen runs: when it began, when a run last saw it, and whether the proxy was told.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Stretch {
    pub since: f64,
    pub seen: f64,
    pub told: bool,
}

/// Why this run is frozen: ready boxes and free slots and no launch (`stopped` is why each planned launch did not happen: the guard that skipped it, or its failure), with the spend guards refusing each account and the first refusal; None when it launched or had nothing to do.
pub fn frozen(outcome: &Outcome, stopped: &[String], guards: &[String]) -> Option<String> {
    let launched = outcome.launches.len() - stopped.len().min(outcome.launches.len());
    if outcome.ready.is_empty() || outcome.free == 0 || launched > 0 {
        return None;
    }
    let first = stopped.first().cloned().or_else(|| outcome.skipped.iter().find(|(_, why)| why.starts_with("ready, not placed")).map(|(item, why)| format!("{item} {why}")));
    let guards = if guards.is_empty() { "no spend guard refuses an account".to_owned() } else { format!("refusing guards: {}", guards.join("; ")) };
    Some(format!("{} ready, {} free slots and no launch; {guards}; first refusal: {}", outcome.ready.len(), outcome.free, first.unwrap_or_else(|| "none recorded".into())))
}

/// The stretch after a run at `at` and the text due to the proxy now. A stretch no run saw within the limit has ended, so the next frozen run starts a new one.
pub fn step(previous: Option<Stretch>, frozen: Option<String>, at: f64, limit_minutes: f64) -> (Option<Stretch>, Option<String>) {
    let limit = limit_minutes * 60.0;
    let Some(basis) = frozen else { return (None, None) };
    let mut stretch = previous.filter(|stretch| at - stretch.seen <= limit).unwrap_or(Stretch { since: at, seen: at, told: false });
    stretch.seen = at;
    if stretch.told || at - stretch.since < limit {
        return (Some(stretch), None);
    }
    stretch.told = true;
    let text = format!("launch freeze: {:.0} min of dispatch runs with {basis}", (at - stretch.since) / 60.0);
    (Some(stretch), Some(text))
}

/// The refusal of each account the spend rule keeps a worker off, with the rule it is.
pub fn guards(survey: &Survey) -> Vec<String> {
    survey.accounts.iter().filter_map(|account| survey.spend.admit(&account.name, Starter::Worker).err().map(|refusal| format!("{refusal} ({})", account.name))).collect()
}

/// Keeps the stretch in `FILE` across runs and tells the proxy once it passes the limit; a failed send is retried on the next frozen run. Returns false when a page due this run did not go out.
pub fn watch(here: &str, frozen: Option<String>, dry_run: bool, at: f64) -> bool {
    let path = record::directory().join(FILE);
    let previous = std::fs::read(&path).ok().and_then(|bytes| serde_json::from_slice::<Stretch>(&bytes).ok());
    let limit = crate::platform::harness::default_number("dispatch_freeze_alarm_min");
    let (mut stretch, text) = step(previous, frozen, at, limit);
    let mut sent_all = true;
    if let Some(text) = text {
        let text = format!("north fleet dispatch on {here}: {text}");
        if dry_run {
            println!("would tell the proxy: {text}");
            return true;
        }
        let north = std::env::current_exe().map(|path| path.to_string_lossy().into_owned()).unwrap_or_else(|_| "north".into());
        let sent = run_within(&north, &["fleet", "send", "--now", "--from", "service:north-dispatch", crate::fleet::runtime::PROXY, &text], Duration::from_secs(30)).is_some_and(|output| output.status.success());
        println!("{}: {text}", if sent { "told the proxy" } else { "could not tell the proxy" });
        sent_all = sent;
        if let Some(stretch) = stretch.as_mut() {
            stretch.told = sent;
        }
    }
    if dry_run {
        return sent_all;
    }
    let written = match &stretch {
        Some(stretch) => std::fs::create_dir_all(record::directory()).and_then(|()| std::fs::write(&path, serde_json::to_vec(stretch).unwrap_or_default())),
        None => std::fs::remove_file(&path).or_else(|error| if error.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(error) }),
    };
    if let Err(error) = written {
        eprintln!("north fleet dispatch: {} not recorded: {error}", path.display());
    }
    sent_all
}
