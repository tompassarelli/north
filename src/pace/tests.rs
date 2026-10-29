use std::fs;

use serde_json::{Value, json};

use super::admission::{interval, is_fresh, refusal};
use super::control::{Control, Reading, advance};
use super::target::{Target, target_in};
use super::window::Window;
use crate::foundation::clock::Timestamp;

struct Policy {
    directory: tempfile::TempDir,
}

impl Policy {
    fn with(text: &str) -> Self {
        let policy = Policy { directory: tempfile::tempdir().unwrap() };
        policy.write(text);
        policy
    }

    fn write(&self, text: &str) {
        fs::write(self.directory.path().join("orchestration.toml"), text).unwrap();
    }

    fn target(&self, account: &str, at: f64) -> Result<Option<Target>, String> {
        target_in(&self.directory.path().join("orchestration.toml"), account, at)
    }

    fn refusal(&self, account: &str, control: &Control, now: f64) -> Option<String> {
        refusal(account, control, now)
    }
}

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

struct Lcg(u64);

impl Lcg {
    fn uniform(&mut self, low: f64, high: f64) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        low + (self.0 >> 11) as f64 / (1u64 << 53) as f64 * (high - low)
    }
}

fn near(left: f64, right: f64) {
    assert!((left - right).abs() < 1e-6, "{left} != {right}");
}

fn reset_week(used: f64, burn: f64, since_start_h: f64, at: f64) -> Window {
    let resets = stamp(at + (168.0 - since_start_h) * 3600.0);
    week(used, Some(burn), 168.0 - since_start_h, &resets, at)
}

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

fn deadline_window(at: f64) -> Window {
    window(json!({"window": "week", "used_pct": 18, "burn_pct_h": 2.02, "reset_h": 157, "resets_at": "reset",
                  "age_s": 0, "as_of": stamp(at), "projected_pct": 335.14}))
}

#[test]
fn deadline_target_admits_spenddown_and_retains_reset_defaults() {
    let at = now();
    let by = stamp(at + 33.0 * 3600.0);
    let policy = Policy::with(&format!("[pace.codex]\nspend_to_pct=90\nby=\"{by}\"\n"));
    let previous = Control { at: Some(at - 60.0), workers: Some(2), allowed_workers: Some(1), held: Some(true), ..Control::default() };
    let target = policy.target("codex", at).unwrap();
    let with_target = |windows, workers, at| Reading { initial: 5, target: target.clone(), ..reading(windows, workers, at) };
    let control = advance(&previous, with_target(vec![deadline_window(at)], Some(2), at));
    assert_eq!(control.held, Some(false));
    assert_eq!(control.allowed_workers, Some(6));
    near(control.windows[0].projected_pct.unwrap(), 84.66);
    assert_eq!(policy.refusal("codex", &control, at), None);
    let native_week = |used: f64, burn: f64| {
        let mut native = deadline_window(at);
        native.resets_at = Some(stamp(at + 157.0 * 3600.0));
        native.used_pct = used;
        native.burn_pct_h = Some(burn);
        native
    };
    let under_line = advance(&previous, with_target(vec![native_week(18.0, 4.83)], Some(5), at));
    assert_eq!(under_line.held, Some(true));
    assert_eq!(policy.refusal("codex", &under_line, at), None);
    let above_line = advance(&previous, Reading { target: target.clone(), ..reading(vec![native_week(24.0, 4.83)], Some(5), at) });
    assert_eq!(above_line.held, Some(true));
    let mut ninety_nine = deadline_window(at);
    ninety_nine.used_pct = 99.0;
    let over = advance(&control, Reading { target: target.clone(), ..reading(vec![ninety_nine], Some(2), at + 60.0) });
    assert_eq!(over.held, Some(true));
    assert_eq!(policy.refusal("codex", &over, at + 60.0), None);
    let claude = step(&previous, vec![deadline_window(at)], Some(2), at);
    assert_eq!(claude.held, Some(true));
    assert_eq!(policy.refusal("claude:c", &claude, at), None);
    assert_eq!(policy.target("codex", at + 34.0 * 3600.0), Ok(None));
    let expired_at = at + 34.0 * 3600.0;
    let mut expired = control.clone();
    expired.at = Some(expired_at);
    expired.windows[0].as_of = Some(stamp(expired_at));
    assert_eq!(policy.refusal("codex", &expired, expired_at), None);
    policy.write("[pace.codex]\nspend_to_pct=90\nby=\"not-a-time\"\n");
    assert!(policy.target("codex", at).is_err());
    assert_eq!(policy.refusal("codex", &control, at), None);
}

#[test]
fn source_timestamp_limits_admission_age() {
    let policy = Policy::with("");
    let mut random = Lcg(120);
    let at = 10_000.0;
    for _ in 0..250 {
        let age = random.uniform(-10.0, 180.0);
        let sample = window(json!({"window": "week", "age_s": 0, "as_of": stamp(at - age), "used_pct": 20, "burn_pct_h": 0,
                                   "reset_h": 100, "projected_pct": 20}));
        let control = Control { at: Some(at - 30.0), held: Some(false), workers: Some(0), allowed_workers: Some(2), windows: vec![sample], ..Control::default() };
        let admitted = (0.0..=120.0).contains(&age);
        assert_eq!(policy.refusal("fixture", &control, at).is_none(), admitted);
        assert_eq!(is_fresh(&control, at), admitted);
    }
}

fn ramp_window(value: f64, label: &str, hours: f64) -> Vec<Window> {
    vec![window(json!({"window": "week", "used_pct": value, "burn_pct_h": 0, "reset_h": hours, "resets_at": "reset",
                       "age_s": 0, "as_of": label, "projected_pct": value}))]
}

#[test]
fn measured_ramp_never_raises_on_missing_or_over_budget() {
    let policy = Policy::with("");
    let mut random = Lcg(61);
    let mut last = Control::default();
    for _ in 0..250 {
        let used = random.uniform(0.0, 50.0);
        let delta = random.uniform(0.001, 2.0);
        let count = random.uniform(1.0, 9.0).floor() as i64;
        let hours = random.uniform(0.1, 200.0);
        let old = step(&Control::default(), ramp_window(used, "first", hours), Some(count), 60.0);
        let new = step(&old, ramp_window(used + delta, "second", hours), Some(count), 660.0);
        near(new.worker_minutes.unwrap(), count as f64 * 10.0);
        near(new.burn_pct_worker_min["week"], delta / (count as f64 * 10.0));
        if used + delta + delta * 6.0 * hours > 100.0 {
            assert_eq!(new.held, Some(true));
            assert_eq!(new.allowed_workers, Some(1));
        } else if used + delta + delta / (count as f64 * 10.0) * 60.0 * 3.0 * hours <= 90.0 {
            assert_eq!(new.allowed_workers, Some(3));
        } else {
            assert_eq!(new.allowed_workers, Some(2));
        }
        let repeated = step(&new, ramp_window(used + delta, "second", hours), Some(count), 670.0);
        assert_eq!(repeated.allowed_workers, new.allowed_workers);
        let missing = step(&new, ramp_window(used + delta, "third", hours), None, 1300.0);
        assert_eq!(missing.held, Some(true));
        assert!(missing.allowed_workers <= new.allowed_workers);
        last = new;
    }
    assert_eq!(interval(60.0, 10.0, false, 0.0), 60.0);
    assert_eq!(interval(60.0, 25.0, false, 0.0), 60.0);
    assert_eq!(interval(60.0, 40.0, false, 0.0), 80.0);
    assert_eq!(interval(60.0, 2.0, true, 180.0), 180.0);
    let stale = Control { at: Some(0.0), ..last };
    assert!(policy.refusal("fixture", &stale, 121.0).unwrap().contains("not fresh"));
}

fn reset_sample(at: f64, used: f64, burn: f64) -> Vec<Window> {
    vec![week(used, Some(burn), 24.0, "unchanged", at)]
}

#[test]
fn a_held_week_admits_until_the_provider_exhausts_it() {
    let policy = Policy::with("");
    let at = now();
    let resets = stamp(at + 12.0 * 3600.0);
    for used in [97.0, 99.0] {
        let control = step(&Control::default(), vec![week(used, None, 12.0, &resets, at)], Some(0), at);
        assert_eq!(control.held, Some(true));
        assert_eq!(policy.refusal("claude:a", &control, at), None);
    }
    let control = step(&Control::default(), vec![week(100.0, None, 12.0, &resets, at)], Some(0), at);
    assert_eq!(policy.refusal("claude:a", &control, at).as_deref(), Some("Hold: claude:a exhausted: provider week 100%."));
}

#[test]
fn a_spawn_is_refused_only_at_the_five_hour_stop_or_over_the_burn_cap() {
    let policy = Policy::with("");
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
    assert_eq!(policy.refusal("claude:b", &held(40.0, 30.0), at), None);
    assert_eq!(
        policy.refusal("claude:b", &held(20.0, 70.0), at).as_deref(),
        Some("Hold: claude:b burning: 5h 70.0%/h > the 50%/h cap that keeps the window two hours.")
    );
    assert_eq!(policy.refusal("claude:b", &held(96.0, 1.0), at).as_deref(), Some("Hold: claude:b exhausted: 5h 96% >= 95%."));
    let other = |name: &str, used: f64| Control {
        at: Some(at),
        windows: vec![window(json!({"window": name, "used_pct": used, "burn_pct_h": 80.0, "reset_h": 4.0, "age_s": 0, "as_of": stamp(at)}))],
        ..Control::default()
    };
    assert_eq!(policy.refusal("codex", &other("43200m", 99.0), at), None);
    assert_eq!(policy.refusal("codex", &other("lim:week", 96.0), at), None);
    assert_eq!(policy.refusal("codex", &other("lim:week", 100.0), at).as_deref(), Some("Hold: codex exhausted: provider lim:week 100%."));
}

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

#[test]
fn soft_hold_resume_flaps_wait_but_a_hard_projection_holds_immediately() {
    let at = now();
    let mut control = step(&Control::default(), reset_sample(at, 30.0, 0.1), Some(1), at);
    for minutes in 1..10 {
        let moment = at + minutes as f64 * 60.0;
        let workers = if minutes % 2 == 1 { None } else { Some(1) };
        control = step(&control, reset_sample(moment, 30.0, 0.1), workers, moment);
        assert_eq!(control.held, Some(false), "{control:?}");
    }
    let moment = at + 600.0;
    control = step(&control, reset_sample(moment, 99.0, 10.0), Some(1), moment);
    assert_eq!(control.held, Some(true));
    control = step(&control, reset_sample(moment + 60.0, 30.0, 0.1), Some(1), moment + 60.0);
    assert_eq!(control.held, Some(true));
}

#[test]
fn target_round_trips_the_policy_entry() {
    let policy = Policy::with("[pace.codex]\nspend_to_pct=90\nby=2099-01-01T00:00:00Z\n");
    let target = policy.target("codex", 0.0).unwrap();
    assert_eq!(target, Some(Target { by: "2099-01-01T00:00:00Z".into(), spend_to_pct: 90.0 }));
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

fn rated(at: f64, workers: i64, allowed: i64, windows: Vec<Window>, rates: &[(&str, f64)]) -> Control {
    Control {
        at: Some(at),
        workers: Some(workers),
        allowed_workers: Some(allowed),
        held: Some(false),
        windows,
        burn_pct_worker_min: rates.iter().map(|(kind, rate)| (kind.to_string(), *rate)).collect(),
        ..Control::default()
    }
}

#[test]
fn a_short_window_burn_caps_the_allowance_at_what_its_per_worker_rate_affords() {
    let at = 1_792_000_000.0;
    let rates = [("week", 0.01000539876691317), ("5h", 0.030016196300739508)];
    let previous = rated(at - 60.0, 20, 20, short_and_week(at - 60.0, 40.0, 15.0, 2.64, 11.0), &rates);
    let control = step(&previous, short_and_week(at, 40.0, 15.0, 2.64, 11.0), Some(20), at);
    assert_eq!(control.allowed_workers, Some(11));
    let next = step(&control, short_and_week(at + 60.0, 40.2, 15.0, 2.63, 11.0), Some(20), at + 60.0);
    assert_eq!(next.allowed_workers, Some(11));
}

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

#[test]
fn a_short_window_projection_past_its_ceiling_caps_workers_without_holding_the_account() {
    let at = 1_792_000_000.0;
    let previous = step(&Control::default(), short_and_week(at - 60.0, 40.0, 50.0, 4.0, 11.0), Some(3), at - 60.0);
    let control = step(&previous, short_and_week(at, 40.5, 50.0, 4.0, 11.0), Some(3), at);
    assert_eq!((control.held, control.over), (Some(false), Some(false)));
}
