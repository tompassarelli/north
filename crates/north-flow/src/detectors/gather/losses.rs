//! What the mechanism-discovery detector reads: the last 7 days' first tries lost to the harness by class, from
//! `north dash losses`, the weekly scoreboard rows that moved for the worse, and the loss keys the turn log
//! holds in its last 28 days, read from origin/main.
use std::collections::BTreeMap;
use std::time::Duration;

use toml_edit::{DocumentMut, Item};

use super::Gatherer;
use crate::detectors::mechanism_discovery::{self, Evidence, Facts, Mechanism, Turn};
use crate::foundation::clock::Timestamp;

const LOSSES_WINDOW: &str = "7d";
const LOSSES_LIMIT: Duration = Duration::from_secs(60);
const WEEKLY_LIMIT: Duration = Duration::from_secs(300);
const CLASSES_PREFIX: &str = "classes: ";
const LOSS_KEY_PREFIX: &str = "first-try-lost:";
const TURN_LOG: &str = "docs/rationale/discovery-turns.toml";
const EXPERIMENTS: &str = "docs/rationale/experiments.toml";
const MECHANISM_PREFIX: &str = "discovery-";
const HELD_DAYS: f64 = 28.0;
const DAY_S: f64 = 86_400.0;

fn classes(stdout: &str) -> Vec<(String, usize)> {
    stdout
        .lines()
        .filter_map(|line| line.strip_prefix(CLASSES_PREFIX))
        .flat_map(|rest| rest.split(", "))
        .filter_map(|part| {
            let (label, count) = part.rsplit_once(' ')?;
            Some((format!("{LOSS_KEY_PREFIX}{label}"), count.parse().ok()?))
        })
        .collect()
}

fn held(turn_log: &str, since: &str) -> Vec<String> {
    let Ok(document) = turn_log.parse::<DocumentMut>() else {
        return Vec::new();
    };
    document
        .get("turn")
        .and_then(Item::as_array_of_tables)
        .into_iter()
        .flat_map(|turns| turns.iter())
        .filter(|turn| turn.get("date").and_then(Item::as_datetime).is_some_and(|date| date.to_string().as_str() >= since))
        .filter_map(|turn| turn.get("loss")?.as_str().map(str::to_owned))
        .collect()
}

impl Gatherer<'_> {
    pub fn mechanism_discovery(&self, now: f64) -> Facts {
        let today = Timestamp::from_unix_seconds_f64(now).iso_date();
        let losses = self.first_try_losses().zip(self.scoreboard_losses()).map(|(mut first_tries, scoreboard)| {
            first_tries.extend(scoreboard);
            first_tries
        });
        let (held, turned) = self.held_turns(now);
        Facts { today, losses, held, turned }
    }

    fn first_try_losses(&self) -> Option<Vec<(String, usize)>> {
        let output = self.host.output(&["north", "dash", "losses", "--since", LOSSES_WINDOW], None, None, LOSSES_LIMIT)?;
        output.status.success().then(|| classes(&String::from_utf8_lossy(&output.stdout)))
    }

    /// The weekly scoreboard's rows that moved for the worse, in runs. The row's better direction comes from
    /// the weekly record; a row it does not hold is left to the first-try source rather than guessed.
    fn scoreboard_losses(&self) -> Option<Vec<(String, usize)>> {
        let output = self.host.output(&["north", "dash", "--weekly"], None, None, WEEKLY_LIMIT).filter(|output| output.status.success())?;
        let weekly = String::from_utf8_lossy(&output.stdout);
        let told = north_graph_loops::scoreboard::read(&self.places.graph).ok()?;
        let directions: BTreeMap<String, bool> = told.readings.into_iter().map(|reading| (reading.number, reading.higher_is_better)).collect();
        Some(mechanism_discovery::scoreboard_losses(&weekly, &directions))
    }

    fn held_turns(&self, now: f64) -> (Vec<String>, Vec<String>) {
        let checkout = self.checkout();
        let path = checkout.to_string_lossy();
        self.host.run(&["git", "-C", &path, "fetch", "-q", "origin"], None, None);
        let shown = self.host.run(&["git", "-C", &path, "show", &format!("origin/main:{TURN_LOG}")], None, None).unwrap_or_default();
        let since = Timestamp::from_unix_seconds_f64(now - HELD_DAYS * DAY_S).iso_date();
        (held(&shown, &since), turn_dates(&shown, &since))
    }

    pub fn discovery_evidence(&self, now: f64) -> Evidence {
        let checkout = self.checkout();
        let path = checkout.to_string_lossy();
        let since = Timestamp::from_unix_seconds_f64(now - HELD_DAYS * DAY_S).iso_date();
        let show = |file: &str| self.host.run(&["git", "-C", &path, "show", &format!("origin/main:{file}")], None, None).unwrap_or_default();
        let turns = turn_dates(&show(TURN_LOG), &since)
            .into_iter()
            .map(|date| {
                let log = self.host.run(&["git", "-C", &path, "log", "origin/main", "--format=%cI", &format!("-Sdate = {date}"), "--", TURN_LOG], None, None).unwrap_or_default();
                Turn { landed: log.lines().last().unwrap_or_default().to_owned(), date }
            })
            .collect();
        let mut bodies: BTreeMap<String, String> = BTreeMap::new();
        let mechanisms = registered(&show(EXPERIMENTS))
            .into_iter()
            .map(|(id, owner)| {
                let body = bodies.entry(owner.clone()).or_insert_with(|| self.issue_body(&owner));
                Mechanism { verdict: body.lines().filter(|line| line.contains(&id)).collect::<Vec<_>>().join("\n") }
            })
            .collect();
        Evidence { turns, mechanisms }
    }

    fn issue_body(&self, owner: &str) -> String {
        let Some((slug, number)) = owner.split_once('#') else {
            return String::new();
        };
        let issue = self.host.json(&["gh", "issue", "view", number, "-R", slug, "--json", "body"]);
        issue.and_then(|issue| issue["body"].as_str().map(str::to_owned)).unwrap_or_default()
    }
}

fn turn_dates(turn_log: &str, since: &str) -> Vec<String> {
    let Ok(document) = turn_log.parse::<DocumentMut>() else {
        return Vec::new();
    };
    let dates = document.get("turn").and_then(Item::as_array_of_tables).into_iter().flat_map(|turns| turns.iter());
    dates.filter_map(|turn| Some(turn.get("date")?.as_datetime()?.to_string())).filter(|date| date.as_str() >= since).collect()
}

fn registered(experiments: &str) -> Vec<(String, String)> {
    let Ok(document) = experiments.parse::<DocumentMut>() else {
        return Vec::new();
    };
    ["experiment", "check"]
        .into_iter()
        .filter_map(|kind| document.get(kind)?.as_array_of_tables())
        .flat_map(|entries| entries.iter())
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?;
            id.starts_with(MECHANISM_PREFIX).then(|| (id.to_owned(), entry.get("owner").and_then(Item::as_str).unwrap_or_default().to_owned()))
        })
        .collect()
}
