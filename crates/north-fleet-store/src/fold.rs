//! Folds a service's alarms of one cause into one message per window: the first alarm of a cause
//! reaches its recipient, while a repeat of that cause within the window and a notice that the alarm
//! cleared or recovered are held for the digest (`held.rs`), except a health unit's one recovered line,
//! the clearing opening the cause again. A cause is the alarm's first sentence with its numbers and
//! commit-shaped tokens blanked, after the ops watch's `still, N min:` prefix, so a re-sent alarm whose load, age or count moved is
//! the same cause. The key blanks a fresh runtime sha and canary session id as well as the numbers, so
//! an alarm re-sent after those moved still folds. Only the alarm services fold, and only the page-now
//! classes in `page.rs` reach the proxy; every other message passes.
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

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
    let pages = to != crate::runtime::PROXY || crate::page::wakes(sender, text);
    let admitted = match cause(text) {
        Cause::Cleared(cause) => {
            let opened = format!("{to}\t{unit}\t{cause}");
            sent.retain(|key, _| !key.starts_with(&opened));
            if pages && unit == "north-health" && cause.starts_with("harness unit ") {
                Admit::Send
            } else {
                Admit::Hold
            }
        }
        Cause::Raised(_) if !pages => Admit::Hold,
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
    crate::foundation::cause::key(text)
}
