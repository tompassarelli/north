//! Folds a service's messages of one condition into one message per episode: the first message of a
//! condition reaches its recipient, while a repeat of it within the window and a notice that it cleared
//! or recovered are held for the digest (`held.rs`), except a health unit's one recovered line and a land-watch
//! page's cleared notice that the grid or the hand-ins answer again, the clearing ending the episode. A service names its condition with a key (`north fleet send --key`), and
//! a new text key under it opens a new episode; an alarm service that names none is keyed by its cause,
//! the alarm's first sentence with its numbers and commit-shaped tokens blanked, after the ops watch's
//! `still, N min:` prefix, so a re-sent alarm whose load, age, count, runtime sha or canary session id
//! moved is the same cause. Only the page-now classes in `page.rs` of the alarm services reach the
//! proxy, every message from a session or Tom passes, and a service's message to the proxy then meets
//! the ceiling in `flood.rs`.
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::alike::{Folded, Join};
use crate::flood::Ceiling;
use crate::sender::Sender;

pub const FILE: &str = "alarm-folds.json";
pub const WINDOW_MS: u64 = 30 * 60 * 1000;
const ALARMS: [&str; 6] = ["north-health", "north-land-watch", "north-ops-flow", "north-capacity-watchdog", "north-lease-reaper", "north-runtime"];
const LANDER: &str = "north-land";
const LANDER_WATCHDOG: &str = "north land watchdog:";
const OPS_WATCH: &str = "north ops watch: ";
const STILL: &str = "still, ";
const CLEARED: &str = "cleared after ";
const RECOVERED: &str = ": recovered";
const HARNESS_UNIT: &str = "harness unit ";
const UNIT_RECOVERED: &str = " recovered: ";
const KEYED: &str = "key:";
const GATE: &str = "[gate ";
const LOCK: &str = "admit.lock";

/// A standing condition a service names when it sends: the store delivers one message per key per
/// episode, and an episode ends when the sender reports it `cleared`, when the message's text key
/// changes, or after [`WINDOW_MS`] in which the sender did not send it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Condition {
    pub key: String,
    pub cleared: bool,
}

/// What becomes of a message: it wakes its recipient, it waits in the digest because it opens nothing
/// new, it is dropped as the clearing of a condition no episode holds open, or it is folded into the
/// page-flood line it carries, which goes out in its place, or it joins the one follow-up of its alike
/// messages (`alike.rs`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Admit {
    Send,
    Hold,
    Drop,
    Flood(String),
    Fold(Folded),
}

#[derive(Debug, Eq, PartialEq)]
enum Cause {
    Raised(String),
    Cleared(String),
}

/// Where a message goes: the addressee it is keyed under, the session it finally reaches (the proxy for a copy re-addressed from an absent lead), and whether a follow-up row is still waiting for the fleet tick.
pub struct Route<'a> {
    pub to: &'a str,
    pub recipient: &'a str,
    pub spooled: &'a dyn Fn(&str) -> bool,
}

/// Whether a message from `sender` to `to` goes out, is held or is folded into a page flood; a
/// delivered alarm or condition is recorded in `root` so its repeats fold into it, and a service's
/// message to the proxy then passes the ceiling in `flood.rs`.
pub fn admit(root: &Path, sender: &Sender, to: &str, text: &str, condition: Option<&Condition>, now_ms: u64) -> Admit {
    admit_via(root, sender, &Route { to, recipient: to, spooled: &|_| false }, text, condition, now_ms)
}

/// As [`admit`], for a message that reaches `route.recipient`: an alike message past the first of its group folds into the group's follow-up before the ceiling counts it.
pub fn admit_via(root: &Path, sender: &Sender, route: &Route, text: &str, condition: Option<&Condition>, now_ms: u64) -> Admit {
    let Sender::Service(_) = sender else { return Admit::Send };
    let _lock = lock(root);
    match fold(root, sender, route.to, text, condition, now_ms) {
        Admit::Send if !text.starts_with(GATE) => {
            if sender.author(text) == *sender
                && let Join::Folds(folded) = crate::alike::join(root, &sender.to_string(), route.recipient, text, now_ms, route.spooled)
            {
                return Admit::Fold(folded);
            }
            if route.to != crate::runtime::PROXY {
                return Admit::Send;
            }
            match crate::flood::admit(root, sender, text, now_ms) {
                Ceiling::Under => Admit::Send,
                Ceiling::Flood(line) => Admit::Flood(line),
                Ceiling::Over => Admit::Hold,
            }
        }
        admitted => admitted,
    }
}

/// Records that `id` is the spooled follow-up of the alike group `key`.
pub fn told(root: &Path, key: &str, id: &str) {
    let _lock = lock(root);
    crate::alike::told(root, key, id);
}

fn lock(root: &Path) -> Option<fs::File> {
    let file = fs::OpenOptions::new().create(true).truncate(false).write(true).open(root.join(LOCK)).ok()?;
    file.lock().ok()?;
    Some(file)
}

fn fold(root: &Path, sender: &Sender, to: &str, text: &str, condition: Option<&Condition>, now_ms: u64) -> Admit {
    let unit = alarm_unit(sender, text);
    let Some((slot, cause)) = opened(to, unit, text, condition) else { return Admit::Send };
    let path = root.join(FILE);
    let mut sent: BTreeMap<String, u64> = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    sent.retain(|_, at| now_ms.saturating_sub(*at) < WINDOW_MS);
    let pages = to != crate::runtime::PROXY || unit.is_none() || crate::page::wakes(sender, text);
    let admitted = match cause {
        Cause::Cleared(cause) => cleared(&mut sent, &format!("{slot}\t{cause}"), condition.is_some(), pages && ((unit == Some("north-health") && text.starts_with(HARNESS_UNIT)) || crate::page::clear_wakes(sender, text))),
        Cause::Raised(_) if !pages => Admit::Hold,
        Cause::Raised(cause) => raised(&mut sent, &slot, &cause, condition.is_some(), now_ms),
    };
    let scratch = path.with_extension("json.part");
    if let Ok(json) = serde_json::to_string(&sent)
        && fs::write(&scratch, json).is_ok()
    {
        let _ = fs::rename(&scratch, &path);
    }
    admitted
}

fn opened(to: &str, unit: Option<&str>, text: &str, condition: Option<&Condition>) -> Option<(String, Cause)> {
    match (condition, unit) {
        (Some(condition), _) if condition.cleared => Some((format!("{to}\t{KEYED}{}", condition.key), Cause::Cleared(String::new()))),
        (Some(condition), _) => Some((format!("{to}\t{KEYED}{}", condition.key), Cause::Raised(blank(first_sentence(text))))),
        (None, Some(unit)) => Some((format!("{to}\t{unit}"), cause(text))),
        (None, None) => None,
    }
}

fn cleared(sent: &mut BTreeMap<String, u64>, opened: &str, keyed: bool, recovered: bool) -> Admit {
    let before = sent.len();
    sent.retain(|key, _| !key.starts_with(opened));
    if keyed && sent.len() == before {
        Admit::Drop
    } else if recovered {
        Admit::Send
    } else {
        Admit::Hold
    }
}

fn raised(sent: &mut BTreeMap<String, u64>, slot: &str, cause: &str, keyed: bool, now_ms: u64) -> Admit {
    let opened = format!("{slot}\t{cause}");
    if keyed {
        let episode = format!("{slot}\t");
        sent.retain(|key, _| *key == opened || !key.starts_with(&episode));
    }
    match sent.insert(opened, now_ms) {
        None => Admit::Send,
        Some(_) => Admit::Hold,
    }
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
    crate::foundation::cause::key(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flood::CLASS;
    use crate::runtime::PROXY;

    #[test]
    fn five_hundred_sends_of_one_condition_from_twelve_senders_deliver_once() {
        let root = tempfile::tempdir().unwrap();
        let senders: Vec<Sender> = (0..12).map(|lander| Sender::Service(format!("north-land@lander-{lander}"))).collect();
        let behind = Condition { key: "lander-build-behind".into(), cleared: false };
        let verdicts: Vec<Admit> = (0..500_u64)
            .map(|send| {
                let text = format!("north land: the lander runs older build cb929a45e{}, {} lander changes behind main 9a896708b{send:x}, over the limit of 3", send % 2, 4 + send % 7);
                admit(root.path(), &senders[send as usize % 12], PROXY, &text, Some(&behind), send * 1000)
            })
            .collect();
        assert_eq!(verdicts.iter().filter(|verdict| **verdict == Admit::Send).count(), 1, "{:?}", &verdicts[..24]);
        assert!(verdicts.iter().all(|verdict| matches!(verdict, Admit::Send | Admit::Hold)), "a repeat of one condition is held, never folded as a flood");

        let lander = &senders[0];
        let distinct: Vec<Admit> = (0..5_u64).map(|condition| admit(root.path(), lander, PROXY, &format!("north land: condition {condition}"), Some(&Condition { key: format!("other-{condition}"), cleared: false }), 590_000 + condition)).collect();
        assert_eq!(&distinct[..2], &[Admit::Send, Admit::Send], "the lander already sent the condition's one message in this window");
        assert!(matches!(&distinct[2], Admit::Flood(line) if line.contains("sent 4 messages") && line.contains(CLASS)), "{distinct:?}");
        assert_eq!(&distinct[3..], &[Admit::Hold, Admit::Hold], "whatever its keys, a sender past the ceiling waits in the digest");

        let starter = Sender::Service("north-land-start".into());
        let units: Vec<String> = (0..7).map(|unit| format!("north-land@repo-{unit}.service")).collect();
        let page = |unit: &str| format!("north land: {unit} has failed to start 16 times and is not serving its queue, so nothing lands there: Unable to locate executable '/lander/tree/bin/north'");
        let rows: Vec<(&str, &str)> = units.iter().map(|unit| (PROXY, unit.as_str())).chain(units[..4].iter().map(|unit| ("land-lead", unit.as_str()))).collect();
        let route = |to: &'static str| Route { to, recipient: PROXY, spooled: &|_| true };
        let folds: Vec<Admit> = rows
            .iter()
            .enumerate()
            .map(|(at, (to, unit))| {
                let key = Condition { key: format!("lander-unit-start:{unit}"), cleared: false };
                let admitted = admit_via(root.path(), &starter, &route(to), &page(unit), Some(&key), 2_000_000 + at as u64);
                if let Admit::Fold(folded) = &admitted {
                    told(root.path(), &folded.key, &format!("row{at}"));
                }
                admitted
            })
            .collect();
        assert_eq!(folds[0], Admit::Send, "the first of the group goes out at once");
        assert!(folds[1..].iter().all(|fold| matches!(fold, Admit::Fold(_))), "nothing is held or folded as a flood: {folds:?}");
        let Admit::Fold(last) = &folds[10] else { unreachable!() };
        assert_eq!(last.replaces.as_deref(), Some("row9"), "each follow-up replaces the one the tick has not yet handed in");
        assert!(last.text.starts_with("north land: * has failed to start 16 times") && last.text.contains(" x 11: "), "{}", last.text);
        assert!(units.iter().all(|unit| last.text.contains(unit.as_str())), "every unit is named: {}", last.text);
        assert!(last.text.contains("repo-0.service /lander/tree/bin/north (x2)") && !last.text.contains("repo-6.service /lander/tree/bin/north (x"), "a copy joins its original: {}", last.text);
    }

    #[test]
    fn four_reports_from_four_sessions_in_ten_minutes_all_reach_the_coordinator() {
        let root = tempfile::tempdir().unwrap();
        let relay = Sender::fleet();
        let report = |worker: &str, turn: u64| format!("[from {worker} · turn {turn} · exit completed] Done: landed it.\nStop: landing");
        let reports: Vec<Admit> = ["north-1-a", "north-2-b", "north-3-c", "north-4-d"].iter().enumerate().map(|(at, worker)| admit(root.path(), &relay, PROXY, &report(worker, 1), None, at as u64 * 120_000)).collect();
        assert_eq!(reports, vec![Admit::Send; 4], "four workers' reports relayed by the fleet each reach the coordinator");

        let repeats: Vec<Admit> = (2..6_u64).map(|turn| admit(root.path(), &relay, PROXY, &report("north-1-a", turn), None, 480_000 + turn)).collect();
        assert_eq!(&repeats[..2], &[Admit::Send, Admit::Send]);
        assert!(matches!(&repeats[2], Admit::Flood(line) if line.contains("session:north-1-a sent 4 messages") && line.contains("`north fleet held --since 10m`")), "{repeats:?}");
        assert_eq!(repeats[3], Admit::Hold, "one session repeating itself is still folded");

        let ticks: Vec<Admit> = (0..4_u64).map(|tick| admit(root.path(), &relay, PROXY, &format!("north fleet: trip {tick} opened"), None, 500_000 + tick)).collect();
        assert!(matches!(&ticks[3], Admit::Flood(line) if line.contains("service:north-fleet sent 4 messages")), "the relay's own messages still count against it: {ticks:?}");
    }
}
