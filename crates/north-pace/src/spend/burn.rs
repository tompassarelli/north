//! The fleet's measured burn on one account: how fast its five-hour and week windows rise, which the
//! burn governor projects to each window's reset.
use serde::{Deserialize, Serialize};

use super::Rules;

/// How long after its five-hour reset an account's week may reset and still count as resetting with it.
const WEEK_RESET_GRACE_S: f64 = 3600.0;

/// One reading of an account's window, in unix seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub at: f64,
    pub used_pct: f64,
    pub resets_at: Option<f64>,
}

/// The fleet's measured burn on one account at `at`, which the burn governor projects to the resets: percent of each window per second, and how many five-hour points one week point costs (zero while the account's rates are unmeasured).
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Burn {
    pub account: String,
    pub at: f64,
    pub five_hour: f64,
    pub week: f64,
    pub five_hour_per_week: f64,
}

/// The latest reading at or before `now` of the window in force then: none when that reading's window has already reset, since no reading of the new window has come in.
fn latest(samples: &[Sample], now: f64) -> Option<&Sample> {
    samples.iter().filter(|sample| sample.at <= now).max_by(|a, b| a.at.total_cmp(&b.at)).filter(|sample| sample.resets_at.is_none_or(|reset| reset > now))
}

/// How far the window in force rose in the last `minutes`: the latest reading less the earliest
/// one inside the span that shares its reset, so a window with one reading since its reset has not risen.
pub fn rise(samples: &[Sample], now: f64, minutes: f64) -> f64 {
    let Some(latest) = latest(samples, now) else { return 0.0 };
    let since = now - minutes * 60.0;
    samples
        .iter()
        .filter(|sample| sample.at >= since && sample.at <= now && sample.resets_at == latest.resets_at)
        .min_by(|a, b| a.at.total_cmp(&b.at))
        .map_or(0.0, |first| (latest.used_pct - first.used_pct).max(0.0))
}

/// Whether the week resets before the five-hour window does, or within `WEEK_RESET_GRACE_S` after it, so exhausting the five-hour window costs nothing the week would keep.
pub fn week_resets_first(week_resets_at: Option<f64>, five_hour_resets_at: Option<f64>, now: f64) -> bool {
    week_resets_at.zip(five_hour_resets_at).is_some_and(|(week, five_hour)| week > now && week <= five_hour + WEEK_RESET_GRACE_S)
}

/// The fleet's burn on `account`: the five-hour rise of the last `spend_burn_five_hour_min` minutes and the week's rise of the last `spend_burn_week_h` hours, each over its own span.
pub fn burn(rules: &Rules, account: &str, five_hour: &[Sample], week: &[Sample], five_hour_per_week: f64, now: f64) -> Burn {
    let per_second = |samples: &[Sample], minutes: f64| rise(samples, now, minutes) / (minutes * 60.0);
    Burn { account: account.to_owned(), at: now, five_hour: per_second(five_hour, rules.five_hour_burn_min), week: per_second(week, rules.week_burn_h * 60.0), five_hour_per_week }
}
