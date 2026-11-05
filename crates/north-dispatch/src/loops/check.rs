use super::{Loop, Role};
use std::time::{Duration, SystemTime};

#[derive(Clone, Debug, PartialEq)]
pub enum Reading {
    At(SystemTime),
    Never,
}

#[derive(Debug, PartialEq)]
pub struct Checked {
    pub lines: Vec<String>,
    pub failing: Vec<String>,
}

pub fn check(loops: &[Loop], held: &dyn Fn(Role) -> bool, read: &dyn Fn(&str) -> Reading, score: &dyn Fn(&str) -> Option<String>, now: SystemTime) -> Checked {
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
    lines.push(format!("loops: {} of {} failing", failing.len(), loops.len()));
    Checked { lines, failing }
}

fn verdict(found: &Loop, held: &dyn Fn(Role) -> bool, read: &dyn Fn(&str) -> Reading, now: SystemTime) -> (String, bool) {
    let (Some(signal), Some(record)) = (&found.signal, &found.record) else {
        return match &found.owner {
            Some(owner) => (format!("open; {} sets it; owner {owner}", found.updated_by), false),
            None => (format!("FAIL no outcome signal and no owner; {} sets it", found.updated_by), true),
        };
    };
    if !held(found.host) {
        return (format!("checked on {}", found.host.name()), false);
    }
    let limit = Duration::from_secs(2 * found.cadence_hours * 3_600);
    match read(record) {
        Reading::Never => (format!("FAIL never updated: no {record}"), true),
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
