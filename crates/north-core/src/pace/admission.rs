use super::control::Control;
use super::exhaustion;
use super::target::seconds;
use super::window::{MAX_AGE_SECONDS, Window};

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
    control.windows.iter().find_map(|w| {
        let why = exhaustion::refusal(&w.window, w.used_pct, w.burn_pct_h)?;
        Some(match resets(w, now) {
            Some(at) => format!("Hold: {account} {why}; the {} window resets at {at}.", w.window),
            None => format!("Hold: {account} {why}."),
        })
    })
}

/// When `window` resets, in local 12-hour time, from its reset stamp or its hours to reset.
pub fn resets(window: &Window, now: f64) -> Option<String> {
    let at = window.resets_at.as_deref().and_then(seconds).or_else(|| (window.reset_h > 0.0).then_some(now + window.reset_h * 3600.0))?;
    Some(crate::foundation::clock::Timestamp::from_unix_seconds_f64(at).local_twelve_hour())
}
