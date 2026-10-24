use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::limits;
use super::target::Target;
use super::trail;
use super::window::{Anchor, MAX_AGE_SECONDS, Window};

const BURN_PERIOD_MINUTES: f64 = 10.0;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Control {
    #[serde(default)]
    pub allowed_workers: Option<i64>,
    #[serde(default)]
    pub held: Option<bool>,
    #[serde(default)]
    pub over: Option<bool>,
    #[serde(default)]
    pub workers: Option<i64>,
    #[serde(default)]
    pub notify: bool,
    #[serde(default)]
    pub notified: Option<bool>,
    #[serde(default)]
    pub worker_minutes: Option<f64>,
    #[serde(default)]
    pub burn_pct_worker_min: BTreeMap<String, f64>,
    #[serde(default)]
    pub sample: Option<Vec<[String; 2]>>,
    #[serde(default)]
    pub at: Option<f64>,
    #[serde(default)]
    pub windows: Vec<Window>,
    #[serde(default)]
    pub target: Option<Target>,
    #[serde(default)]
    pub state_since: Option<f64>,
    #[serde(default)]
    pub changed: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

pub struct Reading {
    pub windows: Vec<Window>,
    pub workers: Option<i64>,
    pub at: f64,
    pub initial: i64,
    pub margin: f64,
    pub target: Option<Target>,
}

struct Measured {
    windows: Vec<Window>,
    rates: BTreeMap<String, f64>,
}

fn measure(previous: &Control, reading: &Reading, worker_minutes: Option<f64>) -> Measured {
    let at = reading.at;
    let prior_at = previous.at.unwrap_or(at);
    let mut windows = Vec::new();
    let mut rates = BTreeMap::new();
    for raw in &reading.windows {
        let mut w = raw.clone();
        let baseline = previous.windows.iter().find(|old| old.window == w.window);
        w.anchor = Some(match baseline.filter(|old| old.resets_at == w.resets_at) {
            Some(old) => {
                let mut anchor = old.anchor.clone().unwrap_or(Anchor { used: old.used_pct, at: prior_at, minutes: 0.0 });
                anchor.minutes += worker_minutes.unwrap_or(0.0);
                let delta = w.used_pct - anchor.used;
                let duration = (at - anchor.at) / 60.0;
                if duration >= BURN_PERIOD_MINUTES && delta >= 0.0 {
                    w.burn_pct_h = Some(delta * 60.0 / duration);
                    if anchor.minutes > 0.0 {
                        rates.insert(w.window.clone(), delta / anchor.minutes);
                    }
                    anchor = Anchor { used: w.used_pct, at, minutes: 0.0 };
                } else if delta >= 0.0 {
                    w.burn_pct_h = old.burn_pct_h;
                }
                anchor
            }
            None => Anchor { used: w.used_pct, at, minutes: 0.0 },
        });
        w.trail = trail::extended(baseline, &w, at, prior_at);
        if let Some(burn) = trail::burn(&w.trail, w.used_pct, at) {
            w.burn_pct_h = Some(burn);
        }
        w.horizon_h = Some(w.horizon(reading.target.as_ref(), at));
        w.projected_pct = w.projected(reading.target.as_ref(), at);
        windows.push(w);
    }
    Measured { windows, rates }
}

fn carried_rates(previous: &Control, windows: &[Window], measured: BTreeMap<String, f64>) -> BTreeMap<String, f64> {
    let mut rates: BTreeMap<String, f64> = previous
        .burn_pct_worker_min
        .iter()
        .filter(|(kind, _)| {
            previous.windows.iter().any(|old| &old.window == *kind)
                && windows.iter().any(|w| &w.window == *kind && previous.windows.iter().any(|old| old.window == w.window && old.resets_at == w.resets_at))
        })
        .map(|(kind, rate)| (kind.clone(), *rate))
        .collect();
    rates.extend(measured);
    rates
}

fn may_raise(windows: &[Window], rates: &BTreeMap<String, f64>, reading: &Reading, allowed: i64) -> bool {
    match &reading.target {
        Some(target) => windows
            .iter()
            .all(|w| w.projected_pct.is_some_and(|p| p < target.spend_to_pct) || w.below_deadline_line(Some(target), reading.at)),
        None => {
            rates.len() == windows.len()
                && windows.iter().all(|w| {
                    rates.get(&w.window).is_some_and(|rate| {
                        w.used_pct + rate * 60.0 * (allowed + 1) as f64 * w.reset_h <= 100.0 - reading.margin
                    })
                })
        }
    }
}

struct Standing {
    held: bool,
    state_since: f64,
    notify: bool,
    notified: Option<bool>,
}

fn standing(previous: &Control, wanted: bool, soft: bool, target_kept: bool, at: f64) -> Standing {
    let dwell = limits::state_dwell_seconds();
    let mut held = wanted;
    let mut state_since = previous.state_since.or(previous.at).unwrap_or(at);
    if let Some(before) = previous.held
        && target_kept && held != before && soft && at - state_since < dwell {
            held = before;
        }
    if Some(held) != previous.held {
        state_since = at;
    }
    let notified = previous.notified.or(previous.held);
    let notify = Some(held) != notified && at - state_since >= dwell;
    Standing { held, state_since, notify, notified: if notify { Some(held) } else { notified } }
}

fn next_allowed(allowed: i64, windows: &[Window], rates: &BTreeMap<String, f64>, reading: &Reading, held: bool, over: bool) -> i64 {
    let known = windows.iter().all(|w| w.projected_pct.is_some());
    if over {
        (allowed / 2).max(1)
    } else if !held && known && reading.workers.is_some_and(|count| count != 0) && may_raise(windows, rates, reading, allowed) {
        allowed + 1
    } else {
        allowed
    }
}

fn is_over(previous: &Control, windows: &[Window], reading: &Reading) -> bool {
    let target = reading.target.as_ref();
    let exhausted = windows.iter().any(|w| w.is_week() && w.used_pct >= w.ceiling(target));
    let ceiling = if previous.over == Some(true) { 100.0 - reading.margin } else { 100.0 };
    exhausted || windows.iter().any(|w| w.projected_pct.is_some_and(|p| p > ceiling))
}

pub fn advance(previous: &Control, mut reading: Reading) -> Control {
    let at = reading.at;
    if reading.target.as_ref().and_then(Target::deadline).is_some_and(|by| by <= at) {
        reading.target = None;
    }
    let target_kept = previous.target == reading.target;
    let started = previous.allowed_workers.unwrap_or(reading.initial);
    let elapsed = (at - previous.at.unwrap_or(at)).max(0.0) / 60.0;
    let worker_minutes = previous
        .workers
        .zip(reading.workers)
        .map(|(old, new)| (old + new) as f64 * elapsed / 2.0);
    let Measured { windows, rates } = measure(previous, &reading, worker_minutes);
    let rates = carried_rates(previous, &windows, rates);
    let sample: Vec<[String; 2]> = windows.iter().map(|w| [w.window.clone(), w.as_of.clone().unwrap_or_default()]).collect();
    let fresh = !windows.is_empty() && windows.iter().all(|w| w.age_s.is_some_and(|age| (0.0..=MAX_AGE_SECONDS).contains(&age)));
    let over = is_over(previous, &windows, &reading);
    let stand = standing(previous, !fresh || over || reading.workers.is_none(), !over && fresh, target_kept, at);
    let mut allowed = if target_kept { started } else { reading.initial };
    if previous.sample.as_ref() != Some(&sample) || !target_kept {
        allowed = next_allowed(allowed, &windows, &rates, &reading, stand.held, over);
    }
    Control {
        changed: Some(stand.held) != previous.held || allowed != started || !target_kept,
        allowed_workers: Some(allowed),
        held: Some(stand.held),
        over: Some(over),
        workers: reading.workers,
        notify: stand.notify,
        notified: stand.notified,
        worker_minutes,
        burn_pct_worker_min: rates,
        sample: Some(sample),
        at: Some(at),
        windows,
        target: reading.target,
        state_since: Some(stand.state_since),
        extra: Map::new(),
    }
}
