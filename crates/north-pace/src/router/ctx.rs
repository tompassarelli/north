use super::moment::Moment;
use super::policy::{Defaults, parse_defaults};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;
use toml_edit::DocumentMut;

static DEFAULTS_TEXT: std::sync::LazyLock<&str> = std::sync::LazyLock::new(|| north_base::platform::harness::table(north_base::platform::harness::DEFAULTS));

#[derive(Debug, Clone)]
pub struct Knobs {
    pub success_threshold: f64,
    pub money_ceiling: Value,
    pub fast_floor: f64,
    pub late: f64,
    pub capacity_min: f64,
    pub codex_floor: Value,
    pub astra_cost: f64,
    pub successor_days: f64,
    pub successor_p: f64,
    pub explore_runs: f64,
    pub min_runs: f64,
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
        money_ceiling: value("money_ceiling"),
        fast_floor: number("fast_floor"),
        late: number("late_ratio"),
        capacity_min: number("capacity_min"),
        codex_floor: value("codex_floor"),
        astra_cost: number("astra_cost"),
        successor_days: number("successor_days"),
        successor_p: number("successor_p"),
        explore_runs: number("explore_runs"),
        min_runs: number("min_runs"),
        codex_explore_min_n: number("codex_explore_min_n"),
        explore_every: number("explore_every") as usize,
        codex: parse_defaults(&DEFAULTS_TEXT).expect("harness/defaults.toml codex classes"),
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
    #[serde(default)]
    pub reset_in: f64,
    pub resets_at: Option<String>,
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
    #[serde(default = "yes")]
    pub auto_route: bool,
    pub provider: Option<String>,
    #[serde(default)]
    pub usd_per_week: f64,
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
    pub u: f64,
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
    pub available_usd: f64,
    pub resets_at: Option<String>,
}

pub fn provider(key: &str) -> &str {
    key.split(':').next().unwrap_or(key)
}

pub fn account_of(tier: &Tier) -> String {
    tier.account
        .clone()
        .unwrap_or_else(|| provider(&tier.key).to_owned())
}

pub fn used(acct: &Account) -> f64 {
    acct.windows
        .values()
        .map(|w| 100.0 - w.headroom)
        .reduce(f64::max)
        .unwrap_or(100.0)
}

pub fn expiring(acct: &Account) -> f64 {
    acct.windows.get("week").map_or(0.0, |room| room.headroom.max(0.0) / room.reset_in.max(1.0 / 60.0))
}

impl Ctx {
    pub fn billing(&self, name: &str, mode: &str) -> &'static str {
        if !self.accounts.contains_key(name) {
            return "unknown";
        }
        let entry = self
            .billing
            .get(name)
            .or_else(|| self.billing.get(provider(name)));
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
        let week = acct.windows.get("week");
        Metrics {
            used_pct: week.map(|week| 100.0 - week.headroom),
            available_usd: week.map_or(0.0, |week| week.headroom.max(0.0) / 100.0 * acct.usd_per_week),
            resets_at: week.and_then(|week| week.resets_at.clone()),
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
        0.0
    }
}
