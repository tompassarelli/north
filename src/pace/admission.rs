use super::control::Control;
use super::exhaustion;
use super::window::MAX_AGE_SECONDS;

const MIN_INTERVAL_SECONDS: f64 = 60.0;
const SLOW_READ_SECONDS: f64 = 20.0;

pub fn is_fresh(control: &Control, at: f64) -> bool {
    (0.0..=MAX_AGE_SECONDS).contains(&(at - control.at.unwrap_or(0.0)))
        && !control.windows.is_empty()
        && control.windows.iter().all(|w| (0.0..=MAX_AGE_SECONDS).contains(&w.age(at)))
}

pub fn interval(previous: f64, elapsed: f64, throttled: bool, retry_after: f64) -> f64 {
    if throttled {
        return MIN_INTERVAL_SECONDS.max(previous * 2.0).max(retry_after);
    }
    if elapsed > SLOW_READ_SECONDS { MIN_INTERVAL_SECONDS.max((elapsed * 2.0).ceil()) } else { MIN_INTERVAL_SECONDS }
}

/// Why a fresh reading keeps a spawn off `account`: a stale sample, or a window the exhaustion rule
/// refuses. The controller's held, over and allowed workers never refuse.
pub fn refusal(account: &str, control: &Control, now: f64) -> Option<String> {
    if !(0.0..=MAX_AGE_SECONDS).contains(&(now - control.at.unwrap_or(0.0))) {
        return Some(format!("Hold: {account} pace sample is not fresh (needs <=120s); refresh before spawning."));
    }
    for w in &control.windows {
        let age = w.age(now);
        if !(0.0..=MAX_AGE_SECONDS).contains(&age) {
            return Some(format!("Hold: {account} {} usage is {age:.0}s old; needs <=120s after refresh.", w.window));
        }
    }
    control
        .windows
        .iter()
        .find_map(|w| exhaustion::refusal(&w.window, w.used_pct, w.burn_pct_h))
        .map(|why| format!("Hold: {account} {why}."))
}
