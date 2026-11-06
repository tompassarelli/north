use serde_json::{Value, json};

use super::admission::refusal;
use super::control::{Control, Reading, advance};
use super::window::Window;
use crate::foundation::clock::Timestamp;

fn stamp(at: f64) -> String {
    Timestamp::from_unix_seconds_f64(at).iso_micros_utc_offset()
}

fn window(value: Value) -> Window {
    serde_json::from_value(value).unwrap()
}

fn week(used: f64, burn: Option<f64>, reset_h: f64, resets_at: &str, at: f64) -> Window {
    window(json!({"window": "week", "used_pct": used, "burn_pct_h": burn, "reset_h": reset_h,
                  "resets_at": resets_at, "age_s": 0, "as_of": stamp(at)}))
}

fn reading(windows: Vec<Window>, workers: Option<i64>, at: f64) -> Reading {
    Reading { windows, workers, at, initial: 2, margin: 10.0, target: None }
}

fn step(previous: &Control, windows: Vec<Window>, workers: Option<i64>, at: f64) -> Control {
    advance(previous, reading(windows, workers, at))
}

fn now() -> f64 {
    Timestamp::now().unix_seconds() as f64
}

fn near(left: f64, right: f64) {
    assert!((left - right).abs() < 1e-6, "{left} != {right}");
}

fn reset_week(used: f64, burn: f64, since_start_h: f64, at: f64) -> Window {
    let resets = stamp(at + (168.0 - since_start_h) * 3600.0);
    week(used, Some(burn), 168.0 - since_start_h, &resets, at)
}

// KEEP(bug): the week projection follows the recent measured burn when it exceeds the week average; regression of a9a336119
#[test]
fn week_projection_follows_recent_burn_when_it_exceeds_the_week_average() {
    let at = now();
    near(reset_week(10.0, 4.87, 2.8, at).projected(None, at).unwrap(), 10.0 + 4.87 * 165.2);
    near(reset_week(40.0, 0.5, 48.0, at).projected(None, at).unwrap(), 40.0 + 0.5 * 120.0);
    near(reset_week(40.0, 44.0, 48.0, at).projected(None, at).unwrap(), 40.0 + 44.0 * 120.0);
    let mut five = reset_week(49.0, 18.3, 3.4, at);
    five.window = "5h".into();
    five.reset_h = 2.1;
    near(five.projected(None, at).unwrap(), 49.0 + 18.3 * 2.1);
    let previous = Control { at: Some(at - 60.0), workers: Some(2), allowed_workers: Some(3), held: Some(false), ..Control::default() };
    let control = advance(&previous, Reading { initial: 3, ..reading(vec![reset_week(40.0, 44.0, 48.0, at)], Some(2), at) });
    assert!(control.windows[0].projected_pct.unwrap() > 100.0);
}

const REPLAY: [[f64; 6]; 59] = [
    [0.0, 15.0, 53.0, 3.0, 10.04, 5.02], [44.0, 15.0, 53.0, 3.0, 10.04, 5.02], [196.0, 16.0, 53.0, 3.0, 10.04, 5.02],
    [223.0, 16.0, 53.0, 3.0, 10.04, 5.02], [362.0, 17.0, 53.0, 4.0, 17.69, 0.0], [472.0, 18.0, 53.0, 5.0, 17.69, 0.0],
    [598.0, 19.0, 53.0, 3.0, 17.69, 0.0], [643.0, 19.0, 53.0, 3.0, 17.69, 0.0], [716.0, 20.0, 54.0, 3.0, 17.69, 0.0],
    [763.0, 20.0, 54.0, 3.0, 17.69, 0.0], [897.0, 21.0, 54.0, 3.0, 17.69, 0.0], [943.0, 21.0, 54.0, 3.0, 17.69, 0.0],
    [1016.0, 21.0, 54.0, 2.0, 22.01, 5.5], [1064.0, 21.0, 54.0, 2.0, 22.01, 5.5], [1141.0, 22.0, 54.0, 2.0, 22.01, 5.5],
    [1264.0, 22.0, 54.0, 3.0, 22.01, 5.5], [1372.0, 23.0, 54.0, 2.0, 22.01, 5.5], [1422.0, 23.0, 54.0, 2.0, 22.01, 5.5],
    [1496.0, 24.0, 54.0, 2.0, 22.01, 5.5], [1544.0, 24.0, 54.0, 2.0, 22.01, 5.5], [1674.0, 24.0, 54.0, 3.0, 16.42, 0.0],
    [1722.0, 24.0, 54.0, 3.0, 16.42, 0.0], [1795.0, 25.0, 54.0, 2.0, 16.42, 0.0], [1843.0, 25.0, 54.0, 2.0, 16.42, 0.0],
    [1975.0, 25.0, 54.0, 2.0, 16.42, 0.0], [2024.0, 25.0, 54.0, 2.0, 16.42, 0.0], [2156.0, 25.0, 54.0, 3.0, 16.42, 0.0],
    [2276.0, 26.0, 55.0, 3.0, 11.97, 5.98], [2393.0, 26.0, 55.0, 3.0, 11.97, 5.98], [2444.0, 26.0, 55.0, 4.0, 11.97, 5.98],
    [2577.0, 28.0, 55.0, 3.0, 11.97, 5.98], [2623.0, 28.0, 55.0, 3.0, 11.97, 5.98], [2695.0, 28.0, 55.0, 3.0, 11.97, 5.98],
    [2807.0, 28.0, 55.0, 3.0, 11.97, 5.98], [2873.0, 29.0, 55.0, 3.0, 11.97, 5.98], [2922.0, 29.0, 55.0, 3.0, 16.71, 0.0],
    [3053.0, 30.0, 55.0, 4.0, 16.71, 0.0], [3172.0, 30.0, 55.0, 3.0, 16.71, 0.0], [3223.0, 30.0, 55.0, 4.0, 16.71, 0.0],
    [3354.0, 31.0, 55.0, 4.0, 16.71, 0.0], [3474.0, 32.0, 56.0, 3.0, 16.71, 0.0], [3526.0, 32.0, 56.0, 3.0, 17.9, 5.97],
    [3655.0, 32.0, 56.0, 3.0, 17.9, 5.97], [3775.0, 33.0, 56.0, 3.0, 17.9, 5.97], [3824.0, 33.0, 56.0, 3.0, 17.9, 5.97],
    [3960.0, 34.0, 56.0, 3.0, 17.9, 5.97], [4003.0, 34.0, 56.0, 3.0, 17.9, 5.97], [4137.0, 34.0, 56.0, 3.0, 11.79, 0.0],
    [4184.0, 34.0, 56.0, 3.0, 11.79, 0.0], [4315.0, 35.0, 56.0, 3.0, 11.79, 0.0], [4363.0, 35.0, 56.0, 4.0, 11.79, 0.0],
    [4422.0, 35.0, 56.0, 4.0, 11.79, 0.0], [4543.0, 35.0, 56.0, 3.0, 11.79, 0.0], [4603.0, 35.0, 56.0, 3.0, 11.79, 0.0],
    [4663.0, 35.0, 56.0, 3.0, 11.79, 0.0], [4728.0, 35.0, 56.0, 3.0, 11.79, 0.0], [4842.0, 35.0, 56.0, 3.0, 5.1, 0.0],
    [4902.0, 35.0, 56.0, 3.0, 5.1, 0.0], [5032.0, 39.0, 57.0, 3.0, 5.1, 0.0],
];

// KEEP(bug): the weekly burn flap of claude:a on 2026-10-16 settles to one held state and one notice; replays recorded samples (c5c8fcc39)
#[test]
fn claude_a_weekly_burn_flap_of_2026_10_16_settles_to_one_state() {
    let start = 1_792_094_497.0;
    let mut previous = Control::default();
    let (mut held, mut notices) = (Vec::new(), 0);
    for [offset, five, week_used, workers, five_burn, week_burn] in REPLAY {
        let at = start + offset;
        let windows = vec![
            window(json!({"window": "5h", "used_pct": five, "burn_pct_h": five_burn, "reset_h": 2.5 - offset / 3600.0,
                          "resets_at": "2026-10-15T22:30:00+00:00", "age_s": 20, "as_of": stamp(at - 20.0)})),
            window(json!({"window": "week", "used_pct": week_used, "burn_pct_h": week_burn, "reset_h": 150.0 - offset / 3600.0,
                          "resets_at": "2026-10-22T02:00:00+00:00", "age_s": 20, "as_of": stamp(at - 20.0)})),
        ];
        previous = step(&previous, windows, Some(workers as i64), at);
        if offset >= 3600.0 {
            held.push(previous.held);
            notices += usize::from(previous.notify);
        }
    }
    let changes = held.windows(2).filter(|pair| pair[0] != pair[1]).count();
    assert!(changes <= 1, "{changes} state changes");
    assert!(notices <= 1, "{notices} notices");
}

// KEEP(bug): a reset sample is not read as a burn of seventy percent per hour; regression of 2fb4089fe
#[test]
fn reset_observation_does_not_turn_one_percent_into_seventy_per_hour() {
    let at = now();
    let reset = stamp(at + 7.0 * 86_400.0 - 60.0);
    let baseline = week(30.0, Some(1.1), 168.0, &reset, at - 52.0);
    let old = step(&Control::default(), vec![baseline.clone()], Some(1), at - 52.0);
    let sample = Window { used_pct: 31.0, burn_pct_h: None, as_of: Some(stamp(at)), ..baseline };
    let new = step(&old, vec![sample], Some(1), at);
    assert_eq!(new.windows[0].burn_pct_h, Some(1.1));
}

// KEEP(bug): a cleared throttle with zero burn recovers headroom from the running workers; regression of the cold-hold recovery (34a3efb20)
#[test]
fn a_cleared_throttle_with_zero_burn_restores_headroom_from_the_running_workers() {
    let at = 1_792_028_893.0;
    let week_at = |at: f64, used: f64, resets: &str, reset_h: f64| {
        window(json!({"window": "week", "used_pct": used, "burn_pct_h": 0.0, "reset_h": reset_h, "resets_at": resets,
                      "age_s": 0, "as_of": stamp(at), "quota_ceiling": 95.0}))
    };
    let previous = Control { at: Some(at - 60.0), workers: Some(21), allowed_workers: Some(2), held: Some(false), ..Control::default() };
    let held = step(&previous, vec![week_at(at, 99.0, "old", 0.003)], Some(21), at);
    assert_eq!((held.held, held.allowed_workers), (Some(true), Some(1)));
    let later = at + 11.0 * 60.0;
    let recovered = step(&held, vec![week_at(later, 1.0, "new", 168.0)], Some(21), later);
    assert_eq!(recovered.held, Some(false));
    assert_eq!(recovered.allowed_workers, Some(22));
}

fn short_and_week(at: f64, short_used: f64, short_burn: f64, short_reset_h: f64, week_used: f64) -> Vec<Window> {
    let stamped = |name: &str, used: f64, burn: f64, reset_h: f64, resets: &str| {
        window(json!({"window": name, "used_pct": used, "burn_pct_h": burn, "reset_h": reset_h, "resets_at": resets,
                      "age_s": 0, "as_of": stamp(at), "quota_ceiling": 95.0}))
    };
    vec![
        stamped("5h", short_used, short_burn, short_reset_h, "short"),
        stamped("week", week_used, 0.5, 160.0, "week"),
    ]
}

// KEEP(mutant): a short-window projection past its ceiling caps workers without holding the account, and the remaining week never does; mutant n259-short-window-projection-holds (34a3efb20); the 2026-11-06 11:45 AM readings held every account at 1 (north#259)
#[test]
fn a_short_window_projection_caps_workers_and_the_remaining_week_never_does() {
    let at = 1_792_000_000.0;
    let previous = step(&Control::default(), short_and_week(at - 60.0, 40.0, 50.0, 4.0, 11.0), Some(3), at - 60.0);
    let control = step(&previous, short_and_week(at, 40.5, 50.0, 4.0, 11.0), Some(3), at);
    assert_eq!((control.held, control.over), (Some(false), Some(false)));
    allowed_workers_follow_the_five_hour_window_and_not_the_remaining_week();
}

// KEEP(mutant): a spawn is refused only at the five-hour stop or over the burn cap; mutant exhaustion-other-window-as-5h (d7da0bbe0)
#[test]
fn a_spawn_is_refused_only_at_the_five_hour_stop_or_over_the_burn_cap() {
    let at = now();
    let five = |used: f64, burn: f64| {
        window(json!({"window": "5h", "used_pct": used, "burn_pct_h": burn, "reset_h": 4.0, "age_s": 0, "as_of": stamp(at)}))
    };
    let held = |used: f64, burn: f64| Control {
        at: Some(at),
        held: Some(true),
        workers: Some(9),
        allowed_workers: Some(0),
        windows: vec![five(used, burn), week(60.0, Some(1.0), 100.0, &stamp(at + 100.0 * 3600.0), at)],
        ..Control::default()
    };
    assert_eq!(refusal("claude:b", &held(40.0, 30.0), at), None);
    let reset = crate::foundation::clock::Timestamp::from_unix_seconds_f64(at + 4.0 * 3600.0).local_twelve_hour();
    assert_eq!(
        refusal("claude:b", &held(20.0, 70.0), at),
        Some(format!("Hold: claude:b burning: 5h 70.0%/h > the 50%/h cap that keeps the window two hours; the 5h window resets at {reset}."))
    );
    assert_eq!(refusal("claude:b", &held(96.0, 1.0), at), Some(format!("Hold: claude:b exhausted: 5h 96% >= 95%; the 5h window resets at {reset}.")));
    let other = |name: &str, used: f64| Control {
        at: Some(at),
        windows: vec![window(json!({"window": name, "used_pct": used, "burn_pct_h": 80.0, "reset_h": 4.0, "age_s": 0, "as_of": stamp(at)}))],
        ..Control::default()
    };
    assert_eq!(refusal("codex", &other("43200m", 99.0), at), None);
    assert_eq!(refusal("codex", &other("lim:week", 96.0), at), None);
    assert_eq!(refusal("codex", &other("lim:week", 100.0), at), Some(format!("Hold: codex exhausted: provider lim:week 100%; the lim:week window resets at {reset}.")));
}

fn weekly_reading(at: f64, five_used: f64, five_reset_h: f64, week_used: f64, week_reset_h: f64) -> Vec<Window> {
    let stamped = |name: &str, used: f64, burn: f64, reset_h: f64, resets: &str| {
        window(json!({"window": name, "used_pct": used, "burn_pct_h": burn, "reset_h": reset_h, "resets_at": resets,
                      "age_s": 0, "as_of": stamp(at), "quota_ceiling": 95.0}))
    };
    vec![
        stamped("5h", five_used, 2.0, five_reset_h, "five"),
        stamped("week", week_used, 0.0, week_reset_h, "week"),
    ]
}

fn allowed_workers_follow_the_five_hour_window_and_not_the_remaining_week() {
    let at = 1_793_000_000.0;
    let climb = |five_used: f64, five_reset_h: f64, week_used: f64, week_reset_h: f64, week_slope: f64| {
        let mut previous = Control::default();
        for tick in 0..25 {
            let moment = at + 60.0 * f64::from(tick);
            let windows = weekly_reading(moment, five_used, five_reset_h, week_used + week_slope * f64::from(tick), week_reset_h);
            previous = advance(&previous, Reading { initial: 1, ..reading(windows, Some(2), moment) });
        }
        previous
    };
    let claude_c = climb(34.0, 3.0, 40.0, 151.0, 0.1);
    assert_eq!((claude_c.held, claude_c.over), (Some(false), Some(false)));
    assert!(claude_c.allowed_workers.unwrap() > 1, "claude:c allowed {:?}", claude_c.allowed_workers);
    let claude_a = climb(0.0, 4.0, 84.0, 141.0, 0.1);
    assert_eq!((claude_a.held, claude_a.over), (Some(false), Some(false)));
    assert!(claude_a.allowed_workers.unwrap() > 1, "claude:a allowed {:?}", claude_a.allowed_workers);
    let spent = climb(0.0, 4.0, 95.0, 141.0, 0.1);
    assert_eq!((spent.held, spent.over), (Some(true), Some(true)));
}
