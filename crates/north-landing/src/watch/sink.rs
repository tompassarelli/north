use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Alert, Severity};

pub const REPEAT_SECONDS: u64 = 600;
pub const DESKTOP_SECONDS: u64 = 300;
const PROXY: &str = "proxy";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Incident {
    pub summary: String,
    pub severity: Severity,
    pub owner: Option<String>,
    pub since: u64,
    pub last_sent: u64,
    pub desktop: bool,
}

pub trait Outbox {
    fn send(&self, to: &str, text: &str) -> bool;
    fn notify(&self, text: &str);
    fn lead_of(&self, session: &str) -> Option<String>;
    fn is_working(&self, session: &str) -> bool;
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
}

impl Outbox for Fleet {
    fn send(&self, to: &str, text: &str) -> bool {
        Command::new("north").args(["fleet", "send", to, text]).status().is_ok_and(|status| status.success())
    }

    fn notify(&self, text: &str) {
        let _ = Command::new("notify-send").args(["--urgency=critical", "north watch", text]).status();
    }

    fn lead_of(&self, session: &str) -> Option<String> {
        let found = self.sessions().iter().find(|entry| entry["name"].as_str() == Some(session))?;
        Some(found["parent"].as_str().filter(|parent| *parent != PROXY).unwrap_or(session).to_owned())
    }

    fn is_working(&self, session: &str) -> bool {
        self.sessions().iter().find(|entry| entry["name"].as_str() == Some(session)).is_some_and(|entry| matches!(entry["status"].as_str(), Some("working" | "starting")))
    }
}

fn recipients(owner: Option<&str>, outbox: &dyn Outbox) -> Vec<String> {
    let mut to = vec![PROXY.to_owned()];
    if let Some(lead) = owner.map(|owner| outbox.lead_of(owner).unwrap_or_else(|| owner.to_owned()))
        && !to.contains(&lead)
        && outbox.is_working(&lead)
    {
        to.push(lead);
    }
    to
}

fn send_all(to: &[String], text: &str, outbox: &dyn Outbox, lines: &mut Vec<String>) {
    for recipient in to {
        let sent = outbox.send(recipient, text);
        lines.push(format!("{} -> {recipient}: {text}", if sent { "sent" } else { "NOT sent" }));
    }
}

pub fn load(path: &Path) -> BTreeMap<String, Incident> {
    fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

pub fn deliver(path: &Path, alerts: &[Alert], unavailable: &[String], now: u64, outbox: &dyn Outbox) -> Vec<String> {
    let mut incidents = load(path);
    let mut lines = Vec::new();
    for alert in alerts {
        step(&mut incidents, alert, now, outbox, &mut lines);
    }
    let ended: Vec<String> = incidents.keys().filter(|key| !alerts.iter().any(|alert| &alert.key == *key) && !unavailable.iter().any(|prefix| key.starts_with(prefix.as_str()))).cloned().collect();
    for key in ended {
        let Some(incident) = incidents.remove(&key) else { continue };
        let minutes = now.saturating_sub(incident.since) / 60;
        if incident.severity > Severity::Info {
            send_all(&recipients(incident.owner.as_deref(), outbox), &format!("north watch: cleared after {minutes} min: {}", incident.summary), outbox, &mut lines);
        }
    }
    if let Ok(text) = serde_json::to_string_pretty(&incidents) {
        let _ = fs::write(path, text + "\n");
    }
    lines
}

fn step(incidents: &mut BTreeMap<String, Incident>, alert: &Alert, now: u64, outbox: &dyn Outbox, lines: &mut Vec<String>) {
    let to = recipients(alert.owner.as_deref(), outbox);
    let entry = incidents.entry(alert.key.clone());
    let incident = match entry {
        std::collections::btree_map::Entry::Vacant(vacant) => {
            if alert.severity > Severity::Info {
                send_all(&to, &format!("north watch: {}", alert.summary), outbox, lines);
            } else {
                lines.push(format!("info: {}", alert.summary));
            }
            vacant.insert(Incident { summary: alert.summary.clone(), severity: alert.severity, owner: alert.owner.clone(), since: alert.since, last_sent: now, desktop: false })
        }
        std::collections::btree_map::Entry::Occupied(occupied) => {
            let incident = occupied.into_mut();
            incident.summary.clone_from(&alert.summary);
            incident.severity = alert.severity;
            incident.owner = alert.owner.clone().or(incident.owner.take());
            if alert.severity > Severity::Info && now.saturating_sub(incident.last_sent) >= REPEAT_SECONDS {
                let minutes = now.saturating_sub(incident.since) / 60;
                send_all(&to, &format!("north watch: still, {minutes} min: {}", alert.summary), outbox, lines);
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
