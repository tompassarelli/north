pub use super::store::Reading;
use super::store::Store;
use super::{Loop, Role};
use std::time::{Duration, SystemTime};

#[derive(Debug, PartialEq)]
pub struct Checked {
    pub lines: Vec<String>,
    pub failing: Vec<String>,
}

pub struct Finding {
    pub name: &'static str,
    pub verdict: String,
    pub failed: bool,
}

pub fn check(loops: &[Loop], held: &dyn Fn(Role) -> bool, read: &dyn Fn(&Store) -> Reading, score: &dyn Fn(&str) -> Option<String>, extra: Vec<Finding>, now: SystemTime) -> Checked {
    let mut lines = Vec::new();
    let mut failing = Vec::new();
    for found in loops {
        let (verdict, failed) = verdict(found, held, read, now);
        if failed {
            failing.push(found.name.clone());
        }
        let scored = found.score.as_deref().filter(|_| held(found.host)).and_then(score).map(|text| format!("; score: {text}")).unwrap_or_default();
        lines.push(format!("{}: {verdict}{scored}", found.name));
    }
    for finding in &extra {
        if finding.failed {
            failing.push(finding.name.to_owned());
        }
        lines.push(format!("{}: {}", finding.name, finding.verdict));
    }
    lines.push(format!("loops: {} of {} failing", failing.len(), loops.len() + extra.len()));
    Checked { lines, failing }
}

fn verdict(found: &Loop, held: &dyn Fn(Role) -> bool, read: &dyn Fn(&Store) -> Reading, now: SystemTime) -> (String, bool) {
    let (Some(signal), Some(store)) = (&found.signal, &found.store) else {
        return match &found.owner {
            Some(owner) => (format!("open; {} sets it; owner {owner}", found.updated_by), false),
            None => (format!("FAIL no outcome signal and no owner; {} sets it", found.updated_by), true),
        };
    };
    if !held(found.host) {
        return (format!("checked on {}", found.host.name()), false);
    }
    let limit = Duration::from_secs(2 * found.cadence_hours * 3_600);
    match read(store) {
        Reading::Never => (format!("FAIL never updated: {}", store.missing()), true),
        Reading::Unread(why) => (format!("FAIL unread: {why}"), true),
        Reading::At(at) => {
            let age = now.duration_since(at).unwrap_or_default();
            let hours = age.as_secs() / 3_600;
            if age > limit {
                (format!("FAIL stale: {} last updated {hours} h ago, over twice its {} h cadence", found.updated_by, found.cadence_hours), true)
            } else {
                (format!("ok: {} updated {hours} h ago from {signal}", found.updated_by), false)
            }
        }
    }
}