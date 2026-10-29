use crate::foundation::clock::Timestamp;
use crate::ops::pacing::placement;
use crate::platform::host;
use expectations::{Expectation, Roster};
use facts::{Machine, Phone};
use serde_json::json;
use std::collections::BTreeMap;

mod expectations;
mod facts;
#[cfg(test)]
mod tests;

const SOURCE: &str = include_str!("../../../north-data/expectations.toml");
const USAGE: &str = "usage: north ops check [--dry-run]";
const STATE: &str = "ops/state.json";

pub(crate) trait Facts {
    fn now(&self) -> f64;
    fn measure(&self, expectation: &Expectation) -> Result<f64, String>;
}

pub(crate) trait Notify {
    fn phone(&mut self, title: &str, text: &str) -> Result<(), String>;
    fn proxy(&mut self, session: &str, text: &str) -> Result<(), String>;
}

pub(crate) type Breaches = BTreeMap<String, f64>;

#[derive(Debug, PartialEq)]
pub(crate) struct Tick {
    pub lines: Vec<String>,
    pub failed: bool,
}

struct Alert<'a> {
    notify: &'a mut dyn Notify,
    proxy: &'a str,
    dry_run: bool,
}

impl Alert<'_> {
    fn send(&mut self, title: &str, text: &str, lines: &mut Vec<String>) -> Delivery {
        if self.dry_run {
            lines.push(format!("  would push the phone and message {}: {title}", self.proxy));
            return Delivery { arrived: false, trouble: false };
        }
        let phone = self.notify.phone(title, text);
        let proxy = self.notify.proxy(self.proxy, &format!("{title}: {text}"));
        if let Err(why) = &phone {
            lines.push(format!("  phone push failed: {why}"));
        }
        if let Err(why) = &proxy {
            lines.push(format!("  proxy message failed: {why}"));
        }
        Delivery { arrived: phone.is_ok() || proxy.is_ok(), trouble: phone.is_err() || proxy.is_err() }
    }
}

struct Delivery {
    arrived: bool,
    trouble: bool,
}

pub(crate) fn tick(roster: &Roster, role: &str, facts: &dyn Facts, notify: &mut dyn Notify, breaches: &mut Breaches, dry_run: bool) -> Tick {
    let mut lines = Vec::new();
    let mut failed = false;
    let mut alert = Alert { notify, proxy: &roster.proxy, dry_run };
    let now = facts.now();
    for expectation in &roster.expectations {
        let name = &expectation.name;
        if expectation.on != role {
            lines.push(format!("{name}: not checked here (runs {})", expectation.on));
            continue;
        }
        if expectation.until.is_some_and(|until| now >= until) {
            lines.push(format!("{name}: retired"));
            breaches.remove(name);
            continue;
        }
        let value = match facts.measure(expectation) {
            Ok(value) => value,
            Err(why) => {
                failed = true;
                lines.push(format!("{name}: unread ({why})"));
                continue;
            }
        };
        let summary = expectation.describe(value);
        match (expectation.breaching(value), breaches.get(name).copied()) {
            (true, None) => {
                lines.push(format!("{name}: BREACH {summary}; owner {}; deterministic fix: {}", expectation.owner, expectation.fix));
                let text = format!("{summary}; owner {}; fix: {}", expectation.owner, expectation.fix);
                let sent = alert.send(&format!("north ops BREACH {name}"), &text, &mut lines);
                failed |= sent.trouble;
                if sent.arrived {
                    breaches.insert(name.clone(), now);
                }
            }
            (true, Some(since)) => lines.push(format!("{name}: still breached since {:.0} min ago ({summary})", (now - since) / 60.0)),
            (false, Some(_)) => {
                lines.push(format!("{name}: CLEARED {summary}"));
                let sent = alert.send(&format!("north ops cleared {name}"), &summary, &mut lines);
                failed |= sent.trouble;
                if sent.arrived {
                    breaches.remove(name);
                }
            }
            (false, None) => lines.push(format!("{name}: ok ({summary})")),
        }
    }
    Tick { lines, failed }
}

fn load() -> Breaches {
    let stored = placement::pace_state(STATE);
    stored["breached"].as_object().into_iter().flatten().filter_map(|(name, since)| Some((name.clone(), since.as_f64()?))).collect()
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let dry_run = match arguments {
        [check] if check == "check" => false,
        [check, flag] if check == "check" && flag == "--dry-run" => true,
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    let roster = match expectations::parse(SOURCE) {
        Ok(roster) => roster,
        Err(why) => {
            eprintln!("north ops check: {why}");
            return 1;
        }
    };
    let directory = placement::pace_directory();
    let host = host::name();
    let role = if host == "nexus" { "nexus" } else { "elsewhere" };
    let machine = Machine { directory: directory.clone(), host: host.clone(), at: Timestamp::now().unix_seconds_f64() };
    let mut breaches = load();
    let outcome = tick(&roster, role, &machine, &mut Phone, &mut breaches, dry_run);
    outcome.lines.iter().for_each(|line| println!("{line}"));
    if dry_run {
        return u8::from(outcome.failed);
    }
    let saved = facts::write_json(&directory.join(STATE), &json!({ "breached": breaches }))
        .and_then(|()| facts::write_heartbeat(&directory, &host, machine.at));
    if let Err(why) = &saved {
        eprintln!("north ops check: state not written: {why}");
    }
    u8::from(outcome.failed || saved.is_err())
}
