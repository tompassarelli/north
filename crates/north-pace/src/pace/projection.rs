//! Placement's projection of an account's windows at a launch's ETA: each window's used percent
//! plus its measured burn over the hours to the ETA or the window's reset, whichever comes first.
//! A launch records it, `north tune` scores it against the percent each window read at the ETA,
//! and dispatch refuses a launch whose week it projects at or past exhaustion before the reset.
use super::exhaustion::{self, WEEK_EXHAUSTED};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// What a usage-gate window reads now: its used percent, measured burn and hours to its reset.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub window: String,
    pub used_pct: f64,
    pub burn_pct_h: Option<f64>,
    pub reset_h: Option<f64>,
}

/// One window's projected used percent `horizon_h` hours after the launch; `at_eta` is false when
/// the window resets before the ETA, so no reading at the ETA can score it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Projected {
    pub window: String,
    pub used_pct: f64,
    pub projected_pct: f64,
    pub horizon_h: f64,
    pub at_eta: bool,
}

/// The windows a launch was placed against, projected at its ETA.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Projection {
    pub eta_h: f64,
    pub windows: Vec<Projected>,
}

pub fn week(window: &str) -> bool {
    window == "week" || window.ends_with(":week")
}

/// The five-hour and week readings of a usage-gate control.
pub fn readings(control: &Value) -> Vec<Reading> {
    control["windows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|window| {
            let name = window["window"].as_str().filter(|name| exhaustion::five_hour(name) || week(name))?;
            Some(Reading { window: name.to_owned(), used_pct: window["used_pct"].as_f64()?, burn_pct_h: window["burn_pct_h"].as_f64(), reset_h: window["reset_h"].as_f64() })
        })
        .collect()
}

impl Reading {
    fn horizon(&self, eta_h: f64) -> f64 {
        self.reset_h.map_or(eta_h, |reset| eta_h.min(reset.max(0.0)))
    }

    /// Points of its own percent the window's measured burn spends before the ETA or its reset.
    pub fn spends(&self, eta_h: f64) -> Option<f64> {
        Some(self.burn_pct_h?.max(0.0) * self.horizon(eta_h))
    }

    pub fn project(&self, eta_h: f64) -> Option<Projected> {
        let spent = self.spends(eta_h)?;
        let horizon_h = self.horizon(eta_h);
        Some(Projected { window: self.window.clone(), used_pct: self.used_pct, projected_pct: self.used_pct + spent, horizon_h, at_eta: horizon_h >= eta_h })
    }
}

/// Each window with a measured burn projected at a launch `eta_h` hours long, None when nothing burns or the ETA is unknown.
pub fn project(readings: &[Reading], eta_h: Option<f64>) -> Option<Projection> {
    let eta_h = eta_h?;
    let windows: Vec<Projected> = readings.iter().filter_map(|reading| reading.project(eta_h)).collect();
    (!windows.is_empty()).then_some(Projection { eta_h, windows })
}

/// Why the week cannot carry a launch: `left` points of the week, after what this run already
/// placed on it, are no more than what its measured burn spends before the ETA or the reset.
pub fn week_refusal(readings: &[Reading], left: f64, eta_h: Option<f64>) -> Option<String> {
    let eta_h = eta_h?;
    let reading = readings.iter().find(|reading| week(&reading.window))?;
    let spent = reading.spends(eta_h)?;
    (spent >= left).then(|| {
        let projected = WEEK_EXHAUSTED - left + spent;
        format!("{} projected {projected:.0}% at {:.1}%/h over {:.0} min before its reset, past {WEEK_EXHAUSTED:.0}%", reading.window, reading.burn_pct_h.unwrap_or_default(), reading.horizon(eta_h) * 60.0)
    })
}
