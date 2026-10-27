use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::limits;
use super::target::{Target, seconds};

const WEEK_SECONDS: f64 = 7.0 * 86_400.0;
pub(super) const MAX_AGE_SECONDS: f64 = 120.0;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Anchor {
    pub used: f64,
    pub at: f64,
    pub minutes: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Window {
    pub window: String,
    pub used_pct: f64,
    #[serde(default)]
    pub burn_pct_h: Option<f64>,
    #[serde(default)]
    pub reset_h: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub as_of: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quota_ceiling: Option<f64>,
    #[serde(default)]
    pub projected_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizon_h: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<Anchor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trail: Vec<[f64; 2]>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Window {
    pub fn is_week(&self) -> bool {
        self.window == "week"
    }

    pub fn age(&self, at: f64) -> f64 {
        self.as_of
            .as_deref()
            .and_then(seconds)
            .map_or(f64::INFINITY, |stamp| at - stamp)
    }

    pub fn ceiling(&self, target: Option<&Target>) -> f64 {
        if target.is_some() {
            limits::plan_stop()
        } else {
            self.quota_ceiling.unwrap_or(100.0 - limits::claude_target_left())
        }
    }

    pub fn horizon(&self, target: Option<&Target>, at: f64) -> f64 {
        match target.and_then(Target::deadline) {
            Some(by) => self.reset_h.min(((by - at) / 3600.0).max(0.0)),
            None => self.reset_h,
        }
    }

    fn week_start(&self) -> Option<f64> {
        self.resets_at.as_deref().and_then(seconds).map(|reset| reset - WEEK_SECONDS)
    }

    pub fn projected(&self, target: Option<&Target>, at: f64) -> Option<f64> {
        self.burn_pct_h.map(|burn| self.used_pct + burn * self.horizon(target, at))
    }

    pub fn below_deadline_line(&self, target: Option<&Target>, at: f64) -> bool {
        let (Some(target), true) = (target, self.is_week()) else {
            return false;
        };
        let (Some(start), Some(end)) = (self.week_start(), target.deadline()) else {
            return false;
        };
        if end == start {
            return false;
        }
        self.used_pct < target.spend_to_pct * ((at - start) / (end - start)).clamp(0.0, 1.0)
    }
}
