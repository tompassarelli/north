use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::json;

use super::exhaustion;
use super::control::{Control, Reading, advance};
use super::target::{Target, seconds};
use super::window::Window;
use crate::foundation::clock::Timestamp;

const GAP_CAP_SECONDS: f64 = 600.0;
const OVERRUN_PCT: f64 = 90.0;
const RESET_JITTER_SECONDS: f64 = 120.0;

#[derive(Deserialize)]
struct Row {
    at: f64,
    workers: Option<i64>,
    #[serde(default)]
    target: Option<Target>,
    windows: Vec<Window>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Verdict {
    False,
    True,
    Unresolved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Cause {
    Stale,
    Floor,
    WeekProjection,
    Dwell,
    Refused,
}

#[derive(Default, Debug)]
struct Tally {
    minutes: BTreeMap<(Cause, bool, bool), f64>,
}

impl Tally {
    fn total(&self, verdict: Verdict) -> f64 {
        self.minutes
            .iter()
            .filter(|((_, truth, unresolved), _)| match verdict {
                Verdict::Unresolved => *unresolved,
                Verdict::True => *truth && !*unresolved,
                Verdict::False => !*truth && !*unresolved,
            })
            .map(|(_, minutes)| minutes)
            .sum()
    }

    fn add(&mut self, cause: Cause, verdict: Verdict, minutes: f64) {
        let key = (cause, verdict == Verdict::True, verdict == Verdict::Unresolved);
        *self.minutes.entry(key).or_default() += minutes;
    }
}

fn load(text: &str) -> Vec<Row> {
    text.lines().map(|line| serde_json::from_str(line).unwrap()).collect()
}

fn week_of(windows: &[Window]) -> Option<&Window> {
    windows.iter().find(|w| w.is_week())
}

fn stale(control: &Control) -> bool {
    control.windows.is_empty() || control.windows.iter().any(|w| !w.age_s.is_some_and(|age| (0.0..=120.0).contains(&age)))
}

fn week_floor(control: &Control) -> bool {
    week_of(&control.windows).is_some_and(|w| w.used_pct >= w.quota_ceiling.unwrap_or(100.0))
}

fn cause(control: &Control, refused: bool) -> Option<Cause> {
    if control.held != Some(true) && !refused {
        return None;
    }
    Some(if stale(control) {
        Cause::Stale
    } else if week_floor(control) {
        Cause::Floor
    } else if control.over == Some(true) {
        Cause::WeekProjection
    } else if refused {
        Cause::Refused
    } else {
        Cause::Dwell
    })
}

fn same_reset(a: &Window, b: &Window) -> bool {
    match (a.resets_at.as_deref().and_then(seconds), b.resets_at.as_deref().and_then(seconds)) {
        (Some(a), Some(b)) => (a - b).abs() < RESET_JITTER_SECONDS,
        _ => a.resets_at == b.resets_at,
    }
}

fn realized_end(rows: &[Row], from: usize, short: &Window) -> f64 {
    rows[from..]
        .iter()
        .take_while(|row| row.windows.iter().any(|w| w.window == short.window && same_reset(w, short)))
        .filter_map(|row| row.windows.iter().find(|w| w.window == short.window))
        .map(|w| w.used_pct)
        .fold(short.used_pct, f64::max)
}

fn judge(rows: &[Row], index: usize, control: &Control, cause: Cause) -> Verdict {
    match cause {
        Cause::Stale | Cause::WeekProjection => Verdict::Unresolved,
        Cause::Floor => Verdict::True,
        Cause::Dwell | Cause::Refused => {
            let Some(short) = control.windows.iter().find(|w| !w.is_week()) else {
                return Verdict::Unresolved;
            };
            let rate = control.burn_pct_worker_min.get(&short.window).copied().unwrap_or(0.0);
            let blocked = rate * short.reset_h * 60.0;
            if realized_end(rows, index, short) + blocked >= OVERRUN_PCT { Verdict::True } else { Verdict::False }
        }
    }
}

fn replay(rows: &[Row]) -> Tally {
    let mut tally = Tally::default();
    let mut control = Control::default();
    for (index, row) in rows.iter().enumerate() {
        let reading = Reading { windows: row.windows.clone(), workers: row.workers, at: row.at, initial: 2, margin: 10.0, target: row.target.clone() };
        control = advance(&control, reading);
        let refused = control.windows.iter().any(|w| exhaustion::refusal(&w.window, w.used_pct, w.burn_pct_h).is_some());
        let Some(cause) = cause(&control, refused) else { continue };
        let minutes = rows.get(index + 1).map_or(0.0, |next| (next.at - row.at).min(GAP_CAP_SECONDS)) / 60.0;
        let verdict = judge(rows, index, &control, cause);
        tally.add(cause, verdict, minutes);
    }
    tally
}

fn climb(account_used: &[(f64, f64)], week_used: f64, workers: i64) -> Vec<Row> {
    let start = Timestamp::now().unix_seconds() as f64 - 7200.0;
    account_used
        .iter()
        .enumerate()
        .map(|(step, (used, burn))| {
            let at = start + step as f64 * 600.0;
            let stamp = Timestamp::from_unix_seconds_f64(at - 20.0).iso_micros_utc_offset();
            let window = |name: &str, used: f64, burn: f64, reset_h: f64| {
                json!({"window": name, "used_pct": used, "burn_pct_h": burn, "reset_h": reset_h, "age_s": 20, "as_of": stamp,
                       "resets_at": format!("reset-{name}"), "quota_ceiling": 95.0})
            };
            serde_json::from_value(json!({"at": at, "workers": workers, "windows": [
                window("5h", *used, *burn, 2.0), window("week", week_used, 0.4, 100.0)
            ]}))
            .unwrap()
        })
        .collect()
}

#[test]
fn three_day_claude_replay_holds_nothing_the_realized_usage_disproves() {
    let rows = load(include_str!("fixtures/claude-c-replay.jsonl"));
    let tally = replay(&rows);
    assert_eq!(tally.total(Verdict::False), 0.0, "{tally:?}");
}

#[test]
fn a_forced_provider_hold_replays_with_zero_false_decisions() {
    let exhausted: Vec<Row> = climb(&[(10.0, 5.0), (12.0, 5.0), (14.0, 5.0)], 100.0, 3);
    let tally = replay(&exhausted);
    assert!(tally.total(Verdict::True) > 0.0, "{tally:?}");
    assert_eq!(tally.total(Verdict::False), 0.0, "{tally:?}");
    assert_eq!(tally.total(Verdict::Unresolved), 0.0, "{tally:?}");
    let spike: Vec<Row> = climb(&[(10.0, 5.0), (96.0, 80.0), (97.0, 80.0)], 40.0, 3);
    let tally = replay(&spike);
    assert_eq!(tally.total(Verdict::False), 0.0, "{tally:?}");
}

#[test]
fn minutes_held_only_by_the_week_projection_are_unresolved_not_true() {
    let mut rows = climb(&[(5.0, 1.0), (5.5, 1.0), (6.0, 1.0)], 60.0, 2);
    for row in &mut rows {
        row.windows[1].burn_pct_h = Some(0.7);
    }
    let tally = replay(&rows);
    assert_eq!(tally.total(Verdict::True), 0.0, "{tally:?}");
    assert_eq!(tally.total(Verdict::False), 0.0, "{tally:?}");
    assert!(tally.total(Verdict::Unresolved) > 0.0, "{tally:?}");
}
