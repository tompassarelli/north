//! Folds a service's alarms of one cause into one message per window: the first alarm of a cause
//! reaches its recipient, while a repeat of that cause within the window and a notice that the alarm
//! cleared or recovered are held for the digest (`held.rs`), the clearing opening the cause again. A
//! cause is the alarm's first sentence with its numbers blanked, after the ops watch's `still, N min:`
//! prefix, so a re-sent alarm whose load, age or count moved is the same cause. Only the alarm
//! services fold; every other message passes.
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::sender::Sender;

pub const FILE: &str = "alarm-folds.json";
pub const WINDOW_MS: u64 = 30 * 60 * 1000;
const ALARMS: [&str; 5] = ["north-health", "north-land-watch", "north-ops-flow", "north-capacity-watchdog", "north-lease-reaper"];
const LANDER: &str = "north-land";
const LANDER_WATCHDOG: &str = "north land watchdog:";
const OPS_WATCH: &str = "north ops watch: ";
const STILL: &str = "still, ";
const CLEARED: &str = "cleared after ";
const RECOVERED: &str = ": recovered";
const HARNESS_UNIT: &str = "harness unit ";
const UNIT_RECOVERED: &str = " recovered: ";

/// What becomes of a message: it wakes its recipient, or it waits in the digest because it opens nothing new.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Admit {
    Send,
    Hold,
}

#[derive(Debug, Eq, PartialEq)]
enum Cause {
    Raised(String),
    Cleared(String),
}

/// Whether a message from `sender` to `to` goes out or is held; a delivered alarm is recorded in `root` so its repeats within [`WINDOW_MS`] fold into it.
pub fn admit(root: &Path, sender: &Sender, to: &str, text: &str, now_ms: u64) -> Admit {
    let Some(unit) = alarm_unit(sender, text) else { return Admit::Send };
    let path = root.join(FILE);
    let mut sent: BTreeMap<String, u64> = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    sent.retain(|_, at| now_ms.saturating_sub(*at) < WINDOW_MS);
    let admitted = match cause(text) {
        Cause::Cleared(cause) => {
            let opened = format!("{to}\t{unit}\t{cause}");
            sent.retain(|key, _| !key.starts_with(&opened));
            Admit::Hold
        }
        Cause::Raised(cause) => match sent.insert(format!("{to}\t{unit}\t{cause}"), now_ms) {
            None => Admit::Send,
            Some(_) => Admit::Hold,
        },
    };
    let scratch = path.with_extension("json.part");
    if let Ok(json) = serde_json::to_string(&sent)
        && fs::write(&scratch, json).is_ok()
    {
        let _ = fs::rename(&scratch, &path);
    }
    admitted
}

/// Whether `text` says an alarm cleared or recovered.
pub fn clears(text: &str) -> bool {
    matches!(cause(text), Cause::Cleared(_))
}

fn alarm_unit<'a>(sender: &'a Sender, text: &str) -> Option<&'a str> {
    let Sender::Service(unit) = sender else { return None };
    let unit = unit.split('@').next().unwrap_or(unit);
    (ALARMS.contains(&unit) || unit == LANDER && text.starts_with(LANDER_WATCHDOG)).then_some(unit)
}

fn cause(text: &str) -> Cause {
    if let Some(alarm) = text.strip_suffix(RECOVERED) {
        return Cause::Cleared(format!("{}:", blank(alarm)));
    }
    if let Some((unit, _)) = text.split_once(UNIT_RECOVERED).filter(|(unit, _)| unit.starts_with(HARNESS_UNIT)) {
        return Cause::Cleared(blank(unit));
    }
    let Some(rest) = text.strip_prefix(OPS_WATCH) else { return Cause::Raised(blank(first_sentence(text))) };
    if let Some(cleared) = rest.strip_prefix(CLEARED) {
        return Cause::Cleared(blank(first_sentence(&format!("{OPS_WATCH}{}", after_minutes(cleared)))));
    }
    let rest = rest.strip_prefix(STILL).map_or(rest, after_minutes);
    Cause::Raised(blank(first_sentence(&format!("{OPS_WATCH}{rest}"))))
}

fn after_minutes(text: &str) -> &str {
    text.split_once(" min: ").map_or(text, |(_, rest)| rest)
}

fn first_sentence(text: &str) -> &str {
    let line = text.lines().next().unwrap_or_default();
    line.find(". ").map_or(line, |end| &line[..end])
}

fn blank(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_number = false;
    for ch in text.trim().chars() {
        let digit = ch.is_ascii_digit();
        if digit && !in_number {
            out.push('#');
        } else if !digit {
            out.extend(ch.to_lowercase());
        }
        in_number = digit;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_same_cause_alarm_folds_within_the_window_and_its_clearing_is_held_and_opens_it_again() {
        let root = tempfile::tempdir().unwrap();
        let (watch, watchdog, lander) = (Sender::Service("north-land-watch@gex45".into()), Sender::Service("north-capacity-watchdog".into()), Sender::Service("north-land".into()));
        let admit = |sender: &Sender, text: &str, at: u64| admit(root.path(), sender, "proxy", text, at) == Admit::Send;

        assert!(admit(&watchdog, "watchdog: host overload, load 107.96 (4.5/core), cpu pressure 72.02% over 60 s. Top process trees: n882-lead2", 0));
        assert!(!admit(&watchdog, "watchdog: host overload, load 98.10 (4.1/core), cpu pressure 64.5% over 60 s. Top process trees: n909-relay", 60_000), "north#909: one overload re-sent with moved numbers");
        assert!(admit(&watchdog, "watchdog: host overload, load 99.1 (4.1/core), cpu pressure 61% over 60 s.", WINDOW_MS), "a new window sends again");
        assert!(admit(&watch, "north ops watch: north main is red: Cargo failed on main commit 4e387709b975", 0));
        assert!(!admit(&watch, "north ops watch: still, 12 min: north main is red: Cargo failed on main commit 4e387709b975", 1));
        assert!(!admit(&watch, "north ops watch: cleared after 13 min: north main is red: Cargo failed on main commit 4e387709b975", 2));
        assert!(admit(&watch, "north ops watch: north main is red: Cargo failed on main commit 4e387709b975", 3), "a cleared cause opens again");
        assert!(admit(&lander, "landed a82759cff8 n909-policy", 0) && admit(&lander, "landed a82759cff8 n909-policy", 1), "a landing notice is no alarm");
    }
}
