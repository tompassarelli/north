use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use north_base::platform::watch_named;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Alert, Severity};

pub const REPEAT_SECONDS: u64 = 600;
pub const DESKTOP_SECONDS: u64 = 300;
const PROXY: &str = "proxy";
const NAMED: usize = 5;
const DETAIL_DIR: &str = "detail";
pub const LINE_CHARS: usize = 300;
const OPENED: &str = "north ops watch: ";
const RED_MAIN: &str = ":red-main:";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Incident {
    pub summary: String,
    pub severity: Severity,
    pub owner: Option<String>,
    pub since: u64,
    pub last_sent: u64,
    pub desktop: bool,
    #[serde(default)]
    pub told: Vec<String>,
    #[serde(default)]
    pub told_summary: String,
}

#[derive(Default)]
pub struct Fleet {
    sessions: OnceCell<Vec<Value>>,
}

impl Fleet {
    fn sessions(&self) -> &[Value] {
        self.sessions.get_or_init(|| {
            let listed = Command::new("north").args(["fleet", "list", "--json"]).output().ok();
            listed.and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok()).and_then(|value| value.as_array().cloned()).unwrap_or_default()
        })
    }

    pub fn send(&self, to: &str, text: &str) -> bool {
        Command::new("north").args(["fleet", "send", "--from", "service:north-land-watch", to, text]).status().is_ok_and(|status| status.success())
    }

    pub fn notify(&self, text: &str) {
        let _ = Command::new("notify-send").args(["--urgency=critical", "north ops watch", text]).status();
    }

    pub fn lead_of(&self, session: &str) -> Option<String> {
        let found = self.sessions().iter().find(|entry| entry["name"].as_str() == Some(session))?;
        Some(found["parent"].as_str().filter(|parent| *parent != PROXY).unwrap_or(session).to_owned())
    }

    pub fn is_working(&self, session: &str) -> bool {
        self.sessions().iter().find(|entry| entry["name"].as_str() == Some(session)).is_some_and(|entry| matches!(entry["status"].as_str(), Some("working" | "starting")))
    }
}

fn recipients(owner: Option<&str>, outbox: &Fleet) -> Vec<String> {
    let mut to = vec![PROXY.to_owned()];
    if let Some(lead) = owner.map(|owner| outbox.lead_of(owner).unwrap_or_else(|| owner.to_owned()))
        && !to.contains(&lead)
        && outbox.is_working(&lead)
    {
        to.push(lead);
    }
    to
}

fn send_all(to: &[String], text: &str, outbox: &Fleet, lines: &mut Vec<String>) {
    for recipient in to {
        let sent = outbox.send(recipient, text);
        lines.push(format!("{} -> {recipient}: {text}", if sent { "sent" } else { "NOT sent" }));
    }
}

pub fn load(path: &Path) -> BTreeMap<String, Incident> {
    fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

pub fn deliver(state: &Path, path: &Path, alerts: &[Alert], unavailable: &[String], now: u64, outbox: &Fleet) -> Vec<String> {
    watch_named::begin(state, now);
    let mut incidents = load(path);
    let mut lines = Vec::new();
    let details = path.parent().unwrap_or(state);
    for alert in alerts {
        step(state, details, &mut incidents, alert, now, outbox, &mut lines);
    }
    let ended: Vec<String> = incidents.keys().filter(|key| !alerts.iter().any(|alert| &alert.key == *key) && !unavailable.iter().any(|prefix| key.starts_with(prefix.as_str()))).cloned().collect();
    for key in ended {
        let Some(incident) = incidents.remove(&key) else { continue };
        let _ = fs::remove_file(detail_file(details, &detail_name(&key)));
        let minutes = now.saturating_sub(incident.since) / 60;
        if incident.severity > Severity::Info {
            send_all(&recipients(incident.owner.as_deref(), outbox), &clip(&format!("{OPENED}cleared after {minutes} min: {}", incident.summary), LINE_CHARS), outbox, &mut lines);
        }
    }
    if let Ok(text) = serde_json::to_string_pretty(&incidents) {
        let _ = fs::write(path, text + "\n");
    }
    lines
}

fn step(state: &Path, details: &Path, incidents: &mut BTreeMap<String, Incident>, alert: &Alert, now: u64, outbox: &Fleet, lines: &mut Vec<String>) {
    let to = recipients(alert.owner.as_deref(), outbox);
    write_detail(details, alert);
    let entry = incidents.entry(alert.key.clone());
    let incident = match entry {
        std::collections::btree_map::Entry::Vacant(vacant) => {
            if alert.severity > Severity::Info {
                let line = news(alert, None, LINE_CHARS - OPENED.len()).unwrap_or_default();
                send_all(&to, &format!("{OPENED}{line}"), outbox, lines);
                watch_named::record(state, now, &alert.summary);
            } else {
                lines.push(format!("info: {}", alert.summary));
            }
            vacant.insert(Incident { summary: alert.summary.clone(), severity: alert.severity, owner: alert.owner.clone(), since: alert.since, last_sent: now, desktop: false, told: if alert.severity > Severity::Info { alert.detail.items.clone() } else { Vec::new() }, told_summary: if alert.severity > Severity::Info { alert.summary.clone() } else { String::new() } })
        }
        std::collections::btree_map::Entry::Occupied(occupied) => {
            let incident = occupied.into_mut();
            incident.summary.clone_from(&alert.summary);
            incident.severity = alert.severity;
            incident.owner = alert.owner.clone().or(incident.owner.take());
            let still = format!("{OPENED}still, {} min: ", now.saturating_sub(incident.since) / 60);
            let line = news(alert, Some(incident), LINE_CHARS.saturating_sub(still.chars().count()));
            if let Some(line) = line.filter(|_| alert.severity > Severity::Info && !alert.key.contains(RED_MAIN) && now.saturating_sub(incident.last_sent) >= REPEAT_SECONDS) {
                send_all(&to, &format!("{still}{line}"), outbox, lines);
                for item in &alert.detail.items {
                    if !incident.told.contains(item) {
                        incident.told.push(item.clone());
                    }
                }
                incident.told_summary.clone_from(&alert.summary);
                watch_named::record(state, now, &alert.summary);
                incident.last_sent = now;
            }
            incident
        }
    };
    if alert.severity == Severity::Page && !incident.desktop && now.saturating_sub(incident.since) >= DESKTOP_SECONDS {
        outbox.notify(&alert.summary);
        lines.push(format!("desktop: {}", alert.summary));
        incident.desktop = true;
    }
}

/// The one line an alert sends now, within `budget` characters: the first time its summary and its items, as many named as fit up to five with their count; after that only a changed summary or the items not told before, and nothing when neither changed. A line with items ends with the command that prints them all.
pub fn news(alert: &Alert, before: Option<&Incident>, budget: usize) -> Option<String> {
    let fresh: Vec<&str> = alert.detail.items.iter().filter(|item| before.is_none_or(|incident| !incident.told.contains(item))).map(String::as_str).collect();
    if fresh.is_empty() && before.is_some_and(|incident| incident.told_summary == alert.summary) {
        return None;
    }
    if fresh.is_empty() {
        return Some(clip(&alert.summary, budget));
    }
    let (detail, more) = (&alert.detail, if before.is_some() { "more " } else { "" });
    let tail = |named: usize| {
        let unnamed = fresh.len() - named;
        let rest = match (named, unnamed) {
            (0, _) => String::new(),
            (_, 0) => format!(" ({})", fresh[..named].join(", ")),
            _ => format!(" ({} and {unnamed} more)", fresh[..named].join(", ")),
        };
        format!("; {} {} {more}{}{rest}; all: north ops watch show {}", detail.lead, fresh.len(), detail.unit, detail_name(&alert.key))
    };
    let fits = (0..=fresh.len().min(NAMED)).rev().map(tail).find(|tail| alert.summary.chars().count() + tail.chars().count() <= budget);
    let tail = fits.unwrap_or_else(|| tail(0));
    Some(format!("{}{tail}", clip(&alert.summary, budget.saturating_sub(tail.chars().count()))))
}

fn clip(text: &str, budget: usize) -> String {
    if text.chars().count() <= budget { text.to_owned() } else { text.chars().take(budget.saturating_sub(1)).collect::<String>() + "…" }
}

/// The name `north ops watch show NAME` reads an alert's detail file by.
pub fn detail_name(key: &str) -> String {
    key.chars().map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' { ch } else { '_' }).collect()
}

pub fn detail_file(dir: &Path, name: &str) -> std::path::PathBuf {
    dir.join(DETAIL_DIR).join(format!("{name}.txt"))
}

fn write_detail(dir: &Path, alert: &Alert) {
    if alert.detail.items.is_empty() {
        return;
    }
    let file = detail_file(dir, &detail_name(&alert.key));
    let text = format!("{}\n{} {} {}:\n{}\n", alert.summary, alert.detail.lead, alert.detail.items.len(), alert.detail.unit, alert.detail.items.join("\n"));
    if let Some(parent) = file.parent()
        && fs::create_dir_all(parent).is_ok()
    {
        let _ = fs::write(&file, text);
    }
}

#[cfg(test)]
#[path = "sink_tests.rs"]
mod tests;
