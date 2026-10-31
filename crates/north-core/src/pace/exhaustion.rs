//! The only usage rule that keeps a launch off an account: its five-hour window at the stop, its
//! measured five-hour burn above the cap that keeps a window at least two hours, or a week the
//! provider reports exhausted. Pace lines, projections, other windows and the controller's held,
//! over and allowed workers never refuse a launch.
use super::limits;

pub const FIVE_HOUR_STOP: f64 = 95.0;
const WEEK_EXHAUSTED: f64 = 100.0;

pub(super) fn five_hour(window: &str) -> bool {
    window == "5h"
}

fn week(window: &str) -> bool {
    window == "week" || window.ends_with(":week")
}

/// Why `window`, `used` percent and measured `burn_pct_h`, refuses a launch, or None.
pub fn refusal(window: &str, used: f64, burn_pct_h: Option<f64>) -> Option<String> {
    if week(window) {
        return (used >= WEEK_EXHAUSTED).then(|| format!("exhausted: provider {window} {used:.0}%"));
    }
    if !five_hour(window) {
        return None;
    }
    if used >= FIVE_HOUR_STOP {
        return Some(format!("exhausted: {window} {used:.0}% >= {FIVE_HOUR_STOP:.0}%"));
    }
    let cap = limits::five_hour_burn_cap();
    burn_pct_h
        .filter(|burn| *burn > cap)
        .map(|burn| format!("burning: {window} {burn:.1}%/h > the {cap:.0}%/h cap that keeps the window two hours"))
}

/// Points left before `window` refuses, which ranks accounts and never refuses one.
pub fn headroom(window: &str, used: f64) -> Option<f64> {
    if week(window) {
        Some(WEEK_EXHAUSTED - used)
    } else {
        five_hour(window).then_some(FIVE_HOUR_STOP - used)
    }
}
