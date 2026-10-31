use super::moment::{Moment, days_to_friday};
use super::policy::{Defaults, parse_defaults};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;
use time::{Duration, Time};
use toml_edit::DocumentMut;

const DEFAULTS_TEXT: &str = include_str!("../../../../harness/defaults.toml");

#[derive(Debug, Clone)]
pub struct Knobs {
    pub success_threshold: f64,
    pub stale_min: f64,
    pub money_ceiling: Value,
    pub plan_stop: Value,
    pub fast_floor: f64,
    pub late: f64,
    pub capacity_min: f64,
    pub codex_floor: Value,
    pub astra_cost: f64,
    pub pace_band: Vec<Value>,
    pub successor_days: f64,
    pub successor_p: f64,
    pub explore_runs: f64,
    pub min_runs: f64,
    pub pace_target_left_claude: f64,
    pub codex_explore_min_n: f64,
    pub explore_every: usize,
    pub codex: Defaults,
}

fn knobs() -> Knobs {
    let document: DocumentMut = DEFAULTS_TEXT.parse().expect("harness/defaults.toml parses");
    let value = |name: &str| super::policy::json_value(&document[name]["value"]);
    let number = |name: &str| {
        value(name)
            .as_f64()
            .unwrap_or_else(|| panic!("harness/defaults.toml has no number for {name}"))
    };
    Knobs {
        success_threshold: number("success_threshold"),
        stale_min: number("usage_stale_min"),
        money_ceiling: value("money_ceiling"),
        plan_stop: value("plan_stop"),
        fast_floor: number("fast_floor"),
        late: number("late_ratio"),
        capacity_min: number("capacity_min"),
        codex_floor: value("codex_floor"),
        astra_cost: number("astra_cost"),
        pace_band: value("pace_band").as_array().cloned().unwrap_or_default(),
        successor_days: number("successor_days"),
        successor_p: number("successor_p"),
        explore_runs: number("explore_runs"),
        min_runs: number("min_runs"),
        pace_target_left_claude: number("pace_target_left_claude"),
        codex_explore_min_n: number("codex_explore_min_n"),
        explore_every: number("explore_every") as usize,
        codex: parse_defaults(DEFAULTS_TEXT).expect("harness/defaults.toml codex classes"),
    }
}

static KNOBS: LazyLock<Knobs> = LazyLock::new(knobs);

impl Default for Knobs {
    fn default() -> Self {
        KNOBS.clone()
    }
}

impl<'de> Deserialize<'de> for Moment {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Moment::parse(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("not an ISO time: {text:?}")))
    }
}

pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Window {
    pub headroom: f64,
    pub beta: Option<f64>,
    #[serde(default)]
    pub reset_in: f64,
    pub resets_at: Option<String>,
    pub window_h: Option<f64>,
    pub as_of: Option<String>,
    pub burn_pct_h: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Account {
    #[serde(default)]
    pub windows: BTreeMap<String, Window>,
    #[serde(default, rename = "hasCredits")]
    pub has_credits: Value,
    #[serde(default)]
    pub extra_usage: Value,
    #[serde(default)]
    pub spend: Value,
    #[serde(default)]
    pub age_min: Value,
    #[serde(default = "yes")]
    pub auto_route: bool,
    pub provider: Option<String>,
    #[serde(default)]
    pub usd_per_week: f64,
    #[serde(default)]
    pub target_left: f64,
    pub band: Option<Vec<Value>>,
    #[serde(default)]
    pub thursday: Value,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Successor {
    pub eta_days: f64,
    pub p: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Tier {
    pub key: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    #[serde(default)]
    pub rank: f64,
    #[serde(default)]
    pub start: f64,
    pub runs: Option<f64>,
    #[serde(default)]
    pub class_runs: BTreeMap<String, f64>,
    pub successor: Option<Successor>,
    pub account: Option<String>,
    #[serde(default)]
    pub fallback: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkBox {
    pub id: String,
    pub project: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub difficulty: Value,
    #[serde(default)]
    pub sensitivity: String,
    #[serde(default)]
    pub local: Value,
    pub brief: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Project {
    pub name: Option<String>,
    pub target: Option<Value>,
    #[serde(default)]
    pub drain: Value,
    #[serde(default)]
    pub u: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Pace {
    #[serde(default)]
    pub enforce_from: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CapacityHit {
    pub since: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ctx {
    pub now: Moment,
    #[serde(default)]
    pub accounts: BTreeMap<String, Account>,
    #[serde(default)]
    pub billing: BTreeMap<String, BTreeMap<String, Value>>,
    #[serde(default)]
    pub tiers: Vec<Tier>,
    #[serde(default)]
    pub projects: BTreeMap<String, Project>,
    #[serde(default)]
    pub boxes: Vec<WorkBox>,
    pub pace: Option<Pace>,
    #[serde(default)]
    pub success: BTreeMap<String, f64>,
    #[serde(default)]
    pub lease: Value,
    #[serde(default)]
    pub capacity: BTreeMap<String, CapacityHit>,
    pub services: Option<Vec<Value>>,
    #[serde(skip)]
    pub knobs: Knobs,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Metrics {
    pub used_pct: Option<f64>,
    pub pace_pct: Option<f64>,
    pub available_usd: f64,
    pub resets_at: Option<String>,
    pub pace_at: Option<String>,
    pub projected_left: Option<f64>,
    pub state: &'static str,
    pub drain_behind: bool,
}

pub const UNDER: &str = "under pace";
pub const OVER: &str = "at or over pace";

pub fn provider(key: &str) -> &str {
    key.split(':').next().unwrap_or(key)
}

pub fn account_of(tier: &Tier) -> String {
    tier.account
        .clone()
        .unwrap_or_else(|| provider(&tier.key).to_owned())
}

pub fn expiry(acct: &Account) -> f64 {
    acct.windows
        .get("week")
        .map_or(f64::INFINITY, |week| week.reset_in)
}

pub fn exhausted(acct: Option<&Account>) -> bool {
    acct.and_then(|a| {
        a.windows
            .get("week")
            .map(|week| week.headroom <= a.target_left)
    })
    .unwrap_or(false)
}

pub fn used(acct: &Account) -> f64 {
    acct.windows
        .values()
        .map(|w| 100.0 - w.headroom)
        .reduce(f64::max)
        .unwrap_or(100.0)
}

fn codex_cutoff(start: Moment) -> Moment {
    let local = start.eastern();
    let friday = local.date() + Duration::days(days_to_friday(local.date().weekday()));
    let mut wall = friday.with_time(Time::from_hms(23, 59, 0).unwrap());
    if wall <= local.wall() {
        wall += Duration::days(7);
    }
    Moment::eastern_wall(wall)
}

fn day_before(cutoff: Moment, codex: bool) -> Moment {
    let wall = cutoff.wall() - Duration::days(1);
    let midnight =
        wall.replace_time(Time::from_hms_nano(0, 0, wall.second(), wall.nanosecond()).unwrap());
    if codex {
        Moment::eastern_wall(midnight)
    } else {
        Moment(midnight.assume_offset(cutoff.0.offset()))
    }
}

fn unmeasured() -> Metrics {
    Metrics {
        used_pct: None,
        pace_pct: None,
        available_usd: 0.0,
        resets_at: None,
        pace_at: None,
        projected_left: None,
        state: OVER,
        drain_behind: false,
    }
}

impl Ctx {
    pub fn stale(&self, acct: &Account) -> bool {
        acct.age_min
            .as_f64()
            .is_none_or(|age| age > self.knobs.stale_min)
    }

    pub fn stale_warning(&self, name: &str, acct: &Account) -> String {
        let age = if acct.age_min.is_null() {
            "unknown".to_owned()
        } else {
            acct.age_min.to_string()
        };
        format!("account {name} STALE: usage {age} minutes old; run agents usage --refresh")
    }

    pub fn billing(&self, name: &str, mode: &str) -> &'static str {
        let Some(acct) = self.accounts.get(name) else {
            return "unknown";
        };
        let entry = self
            .billing
            .get(name)
            .or_else(|| self.billing.get(provider(name)));
        if self.stale(acct) {
            return "unknown";
        }
        match entry
            .and_then(|modes| modes.get(mode))
            .and_then(Value::as_str)
        {
            Some("plan") => "plan",
            Some("money") => "money",
            Some("blocked") => "blocked",
            _ => "unknown",
        }
    }

    pub fn overflow_money(&self, name: &str) -> bool {
        let acct = &self.accounts[name];
        let live = truthy(&acct.has_credits) || truthy(&acct.extra_usage) || truthy(&acct.spend);
        live || !matches!(self.billing(name, "overflow"), "plan" | "blocked")
    }

    pub fn metrics(&self, acct: &Account) -> Metrics {
        let now = self.now;
        let Some(week) = acct.windows.get("week") else {
            return unmeasured();
        };
        let Some(reset) = week.resets_at.as_deref().and_then(Moment::parse) else {
            return unmeasured();
        };
        let start = reset.plus_seconds(-week.window_h.unwrap_or(0.0) * 3600.0);
        let codex = acct.provider.as_deref() == Some("codex");
        let cutoff = if codex { codex_cutoff(start) } else { reset };
        let duration = cutoff.seconds_since(start);
        let elapsed = if duration > 0.0 {
            (now.seconds_since(start) / duration).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let pace_pct = (100.0 - acct.target_left) * elapsed;
        let used_pct = 100.0 - week.headroom;
        let observed = week.as_of.as_deref().and_then(Moment::parse);
        let projected = week.beta.zip(observed).map(|(rate, observed)| {
            let remaining_h = (cutoff.seconds_since(observed) / 3600.0).max(0.0);
            (week.headroom - rate * remaining_h).clamp(0.0, 100.0)
        });
        let thursday = day_before(cutoff, codex);
        let band = acct
            .band
            .clone()
            .unwrap_or_else(|| self.knobs.pace_band.clone());
        let top = band.get(1).and_then(Value::as_f64).unwrap_or(f64::INFINITY);
        Metrics {
            used_pct: Some(used_pct),
            pace_pct: Some(pace_pct),
            available_usd: week.headroom.max(0.0) / 100.0 * acct.usd_per_week,
            resets_at: week.resets_at.clone(),
            pace_at: Some(cutoff.iso()),
            projected_left: projected,
            state: if used_pct < pace_pct { UNDER } else { OVER },
            drain_behind: truthy(&acct.thursday)
                && now >= thursday
                && projected.is_some_and(|p| p > top),
        }
    }

    pub fn price(&self, name: &str) -> f64 {
        if self.billing(name, "standard") != "plan" {
            return f64::INFINITY;
        }
        let acct = &self.accounts[name];
        if used(acct) >= self.knobs.money_ceiling.as_f64().unwrap_or(f64::INFINITY)
            && self.overflow_money(name)
        {
            return f64::INFINITY;
        }
        if self.metrics(acct).state == UNDER {
            0.0
        } else {
            1.0
        }
    }

    pub fn account_state(&self, name: &str) -> Option<&'static str> {
        self.accounts.get(name).map(|acct| self.metrics(acct).state)
    }
}
