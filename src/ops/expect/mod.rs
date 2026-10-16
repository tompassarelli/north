use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::facts::{DAY, Facts, parse};

mod metric;
#[cfg(test)]
mod tests;

pub(crate) use metric::{Metric, Span};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum Op {
    #[serde(rename = ">=")]
    AtLeast,
    #[serde(rename = ">")]
    Above,
    #[serde(rename = "<=")]
    AtMost,
    #[serde(rename = "<")]
    Below,
    #[serde(rename = "==")]
    Equals,
}

impl Op {
    fn holds(self, value: f64, threshold: f64) -> bool {
        match self {
            Op::AtLeast => value >= threshold,
            Op::Above => value > threshold,
            Op::AtMost => value <= threshold,
            Op::Below => value < threshold,
            Op::Equals => (value - threshold).abs() < f64::EPSILON,
        }
    }

    pub(crate) fn symbol(self) -> &'static str {
        match self {
            Op::AtLeast => ">=",
            Op::Above => ">",
            Op::AtMost => "<=",
            Op::Below => "<",
            Op::Equals => "==",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub(crate) struct Expectation {
    pub id: String,
    pub behavior: String,
    pub metric: String,
    pub op: Op,
    pub threshold: f64,
    #[serde(default)]
    pub window: Option<String>,
    #[serde(default)]
    pub until: Option<String>,
    pub owner: String,
    pub fix: String,
    #[serde(default)]
    pub params: BTreeMap<String, f64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum State {
    Holds,
    Breached,
    Unknown,
    Inactive,
}

impl State {
    pub(crate) fn label(self) -> &'static str {
        match self {
            State::Holds => "HOLDS",
            State::Breached => "BREACHED",
            State::Unknown => "UNKNOWN",
            State::Inactive => "INACTIVE",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Outcome {
    pub id: String,
    pub behavior: String,
    pub metric: String,
    pub value: Option<f64>,
    pub op: Op,
    pub threshold: f64,
    pub window: String,
    pub state: State,
    pub owner: String,
    pub fix: String,
}

impl Outcome {
    pub(crate) fn line(&self) -> String {
        let value = self.value.map_or("unknown".to_owned(), |value| format!("{value:.2}"));
        format!(
            "{} {}: {} ({} = {value}, expected {} {} over {}; owner {})",
            self.id,
            self.state.label(),
            self.behavior,
            self.metric,
            self.op.symbol(),
            self.threshold,
            self.window,
            self.owner
        )
    }
}

pub(crate) fn load(text: &str) -> Result<Vec<Expectation>, String> {
    let document: toml_edit::DocumentMut = text.parse().map_err(|error| format!("expectations: {error}"))?;
    let tables = document.get("expectation").and_then(toml_edit::Item::as_array_of_tables).ok_or("expectations: no [[expectation]] tables")?;
    let expectations: Vec<Expectation> = tables
        .iter()
        .map(|table| serde_json::from_value(table_json(table)).map_err(|error| format!("expectations: {error}")))
        .collect::<Result<_, _>>()?;
    for expectation in &expectations {
        if Metric::named(&expectation.metric).is_none() {
            return Err(format!("expectations: {} names unknown metric {}", expectation.id, expectation.metric));
        }
    }
    Ok(expectations)
}

fn table_json(table: &toml_edit::Table) -> Value {
    Value::Object(table.iter().filter_map(|(key, item)| Some((key.to_owned(), item_json(item)?))).collect())
}

fn item_json(item: &toml_edit::Item) -> Option<Value> {
    match item {
        toml_edit::Item::Value(value) => Some(value_json(value)),
        toml_edit::Item::Table(table) => Some(table_json(table)),
        _ => None,
    }
}

fn value_json(value: &toml_edit::Value) -> Value {
    match value {
        toml_edit::Value::String(text) => Value::from(text.value().as_str()),
        toml_edit::Value::Integer(number) => Value::from(*number.value() as f64),
        toml_edit::Value::Float(number) => Value::from(*number.value()),
        toml_edit::Value::Boolean(flag) => Value::from(*flag.value()),
        toml_edit::Value::Array(items) => Value::Array(items.iter().map(value_json).collect()),
        toml_edit::Value::InlineTable(table) => Value::Object(table.iter().map(|(key, value)| (key.to_owned(), value_json(value))).collect()),
        toml_edit::Value::Datetime(at) => Value::from(at.value().to_string()),
    }
}

pub(crate) fn window_seconds(window: Option<&str>) -> i64 {
    let window = window.unwrap_or("24h").trim();
    if window == "now" {
        return 0;
    }
    let (number, unit) = window.split_at(window.len().saturating_sub(1));
    let number: i64 = number.parse().unwrap_or(24);
    number
        * match unit {
            "m" => 60,
            "h" => 3600,
            "d" => DAY,
            _ => 3600,
        }
}

pub(crate) fn evaluate(expectation: &Expectation, facts: &Facts) -> Outcome {
    evaluate_at(expectation, facts, facts.now)
}

pub(crate) fn evaluate_at(expectation: &Expectation, facts: &Facts, end: i64) -> Outcome {
    let metric = Metric::named(&expectation.metric);
    let expired = expectation.until.as_deref().and_then(parse::seconds).is_some_and(|until| end >= until);
    let span = Span { from: end - window_seconds(expectation.window.as_deref()), to: end };
    let current = end == facts.now;
    let value = metric.filter(|metric| !expired && (metric.windowed() || current)).and_then(|metric| metric.measure(facts, &expectation.params, &span));
    let state = match value {
        _ if expired => State::Inactive,
        Some(value) if expectation.op.holds(value, expectation.threshold) => State::Holds,
        Some(_) => State::Breached,
        None => State::Unknown,
    };
    Outcome {
        id: expectation.id.clone(),
        behavior: expectation.behavior.clone(),
        metric: expectation.metric.clone(),
        value,
        op: expectation.op,
        threshold: expectation.threshold,
        window: expectation.window.clone().unwrap_or_else(|| "24h".into()),
        state,
        owner: expectation.owner.clone(),
        fix: expectation.fix.clone(),
    }
}
