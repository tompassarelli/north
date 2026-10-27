use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};
use toml_edit::DocumentMut;

use super::{int, truthy};
use crate::foundation::clock::Timestamp;

static HANDOFF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:Your context is \d+k tokens, past the \d+k handoff point\.|You have run \d+ minutes\. Land anything that passes now)").unwrap()
});
static REPLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:HANDOFF\s+\S+|(?:Not done|Done):)").unwrap());
static REMINDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^<system-reminder>\s*").unwrap());

const SYNTHETIC: &str = "<synthetic>";

pub(crate) struct Prices {
    rates: HashMap<String, (f64, f64)>,
    by_price: bool,
    efforts: Vec<(String, Value)>,
}

impl Prices {
    pub(crate) fn parse(catalog: &str, defaults: &str) -> Self {
        let catalog: DocumentMut = catalog.parse().expect("harness/model-catalog.toml parses");
        let defaults: DocumentMut = defaults.parse().expect("harness/defaults.toml parses");
        let by_price = defaults.get("plan_meter_weighting").and_then(|entry| entry.get("value")).and_then(|value| value.as_str()) == Some("price");
        let mut rates = HashMap::new();
        let mut efforts = Vec::new();
        for (_, row) in catalog.get("models").and_then(|models| models.as_table_like()).into_iter().flat_map(|table| table.iter()) {
            let Some(model) = row.get("model").and_then(|model| model.as_str()) else { continue };
            efforts.push((model.to_owned(), row.get("effort").and_then(|effort| effort.as_str()).map_or(Value::Null, Value::from)));
            let api = row.get("value").and_then(|value| value.get("api"));
            let rate = |key: &str| api.and_then(|api| api.get(key)).and_then(|price| price.as_float().or_else(|| price.as_integer().map(|price| price as f64)));
            if let (Some(input), Some(output)) = (rate("input_per_million"), rate("output_per_million")) {
                rates.insert(model.to_owned(), (input, output));
            }
        }
        Self { rates, by_price, efforts }
    }

    pub(crate) fn weight(&self, model: Option<&str>, tokens: [f64; 4]) -> (f64, bool) {
        let [input, write, read, output] = tokens;
        match model.and_then(|model| self.rates.get(model)).filter(|_| self.by_price) {
            Some((rate_in, rate_out)) => (input * rate_in + write * rate_in * 1.25 + read * rate_in * 0.1 + output * rate_out, true),
            None => {
                let read_share = if model.unwrap_or("").to_lowercase().contains("opus") { 0.05 } else { 0.1 };
                (input + write * 1.25 + read * read_share + output * 5.0, false)
            }
        }
    }

    pub(crate) fn lists(&self, model: &str) -> bool {
        self.rates.contains_key(model)
    }

    pub(crate) fn by_price(&self) -> bool {
        self.by_price
    }

    pub(crate) fn effort(&self, model: Option<&str>) -> Value {
        self.efforts.iter().find(|(name, _)| Some(name.as_str()) == model).map_or(Value::Null, |(_, effort)| effort.clone())
    }
}

pub(crate) fn record_text(record: &Value) -> String {
    let body = ["message", "payload", "attachment"].iter().map(|key| &record[*key]).find(|value| truthy(value)).unwrap_or(record);
    let content = ["content", "prompt", "text"].iter().map(|key| &body[*key]).find(|value| truthy(value)).unwrap_or(&Value::Null);
    match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| match part {
                Value::String(text) => Some(text.clone()),
                Value::Object(_) => Some(
                    part["text"].as_str().filter(|text| !text.is_empty()).or_else(|| part["content"].as_str()).unwrap_or("").to_owned(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

pub(crate) fn hook_fire(record: &Value) -> bool {
    let text = record_text(record);
    let text = text.trim();
    let text = match serde_json::from_str::<Value>(text) {
        Ok(result) => result["hookSpecificOutput"]["additionalContext"].as_str().unwrap_or("").to_owned(),
        Err(_) => {
            if record["message"]["content"].as_array().is_some_and(|parts| parts.iter().any(|part| part["type"] == "tool_result")) {
                return false;
            }
            text.to_owned()
        }
    };
    HANDOFF.is_match(&REMINDER.replace(&text, ""))
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Text(String),
    Index(usize),
}

fn request_key(record: &Value, index: usize) -> Key {
    [&record["requestId"], &record["message"]["id"]]
        .into_iter()
        .find(|value| truthy(value))
        .map_or(Key::Index(index), |value| Key::Text(value.as_str().map_or_else(|| value.to_string(), str::to_owned)))
}

pub(crate) struct Request {
    pub(crate) at: Option<String>,
    pub(crate) total: i64,
    pub(crate) cache: i64,
}

pub(super) fn context_metrics(records: &[Value]) -> Map<String, Value> {
    let (mut boundaries, mut summaries, mut fires) = (0, 0, Vec::new());
    let mut order: Vec<Key> = Vec::new();
    let mut requests: HashMap<Key, Request> = HashMap::new();
    for (index, record) in records.iter().enumerate() {
        let kind = record["type"].as_str().unwrap_or("");
        let summary = truthy(&record["isCompactSummary"]);
        boundaries += usize::from(kind == "system" && record["subtype"] == "compact_boundary");
        summaries += usize::from(summary);
        if matches!(kind, "user" | "system" | "attachment") && !summary && hook_fire(record) {
            fires.push(index);
        }
        let usage = &record["message"]["usage"];
        if kind != "assistant" || usage.is_null() {
            continue;
        }
        let cache = int(&usage["cache_read_input_tokens"]);
        let total = int(&usage["input_tokens"]) + cache + int(&usage["cache_creation_input_tokens"]);
        let at = record["timestamp"].as_str().map(str::to_owned);
        let key = request_key(record, index);
        match requests.get_mut(&key) {
            None => {
                order.push(key.clone());
                requests.insert(key, Request { at, total, cache });
            }
            Some(previous) if total > previous.total => {
                previous.total = total;
                previous.cache = cache;
            }
            Some(_) => {}
        }
    }
    let mut values: Vec<&Request> = order.iter().map(|key| &requests[key]).collect();
    values.sort_by(|left, right| left.at.as_deref().unwrap_or("").cmp(right.at.as_deref().unwrap_or("")));
    let mut out = Map::new();
    out.insert("peak_ctx".into(), json!(values.iter().map(|value| value.total).max().unwrap_or(0)));
    out.insert("compactions".into(), json!(boundaries.max(summaries)));
    out.insert("handoff_fires".into(), json!(fires.len()));
    out.insert("handoff_ignored".into(), json!(fires.iter().filter(|fire| !answered(records, **fire)).count()));
    out.insert("input_tokens".into(), json!(values.iter().map(|value| value.total).sum::<i64>()));
    out.insert("cache_read_tokens".into(), json!(values.iter().map(|value| value.cache).sum::<i64>()));
    out.insert("idle_gap_misses".into(), json!(idle_gap_misses(&values)));
    out
}

pub(crate) fn idle_gap_misses(values: &[&Request]) -> usize {
    let (mut misses, mut previous) = (0, None);
    for value in values {
        let Some(at) = value.at.as_deref().filter(|at| !at.is_empty()).and_then(|at| Timestamp::parse(at).ok()) else { continue };
        let at = at.unix_seconds_f64();
        if previous.is_some_and(|before: f64| at - before > 3600.0) && value.total > 0 && value.cache as f64 / value.total as f64 <= 0.05 {
            misses += 1;
        }
        previous = Some(at);
    }
    misses
}

fn answered(records: &[Value], fire: usize) -> bool {
    let mut turns: Vec<(Key, String)> = Vec::new();
    for (index, record) in records.iter().enumerate().skip(fire + 1) {
        if record["type"] != "assistant" {
            continue;
        }
        let key = request_key(record, index);
        match turns.iter().position(|(seen, _)| *seen == key) {
            Some(position) => turns[position].1.push_str(&record_text(record)),
            None if turns.len() == 3 => break,
            None => turns.push((key, record_text(record))),
        }
    }
    turns.iter().any(|(_, text)| REPLY.is_match(text))
}

pub(super) fn run_usage(records: &[Value], account: &str, prices: &Prices) -> Map<String, Value> {
    let (mut model, mut effort, mut fast): (Option<&str>, Value, bool) = (None, Value::Null, false);
    let mut samples: Vec<Value> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    let mut unpriced = BTreeSet::new();
    for (index, record) in records.iter().enumerate() {
        let message = &record["message"];
        let usage = &message["usage"];
        if record["type"] != "assistant" || usage.is_null() {
            continue;
        }
        model = message["model"].as_str().filter(|name| !name.is_empty() && *name != SYNTHETIC).or(model);
        if let Some(value) = [&message["effort"], &record["effort"]].into_iter().find(|value| truthy(value)) {
            effort = value.clone();
        }
        fast = usage["speed"] == "fast";
        let key = match request_key(record, index) {
            Key::Text(text) => text,
            Key::Index(index) => index.to_string(),
        };
        let Some(at) = record["timestamp"].as_str().filter(|at| !at.is_empty()) else { continue };
        let tokens = ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens", "output_tokens"].map(|name| int(&usage[name]) as f64);
        let (weight, priced) = prices.weight(model, tokens);
        if weight != 0.0 && !priced && prices.by_price {
            unpriced.insert(model.unwrap_or("unknown").to_owned());
        }
        let sample = json!({"request": key, "ts": at, "weight": weight});
        match positions.get(&key) {
            Some(&position) if weight > samples[position]["weight"].as_f64().unwrap_or(0.0) => samples[position] = sample,
            Some(_) => {}
            None => {
                positions.insert(key, samples.len());
                samples.push(sample);
            }
        }
    }
    if effort.is_null() {
        effort = prices.effort(model);
    }
    let mut out = Map::new();
    out.insert("account".into(), json!(account));
    out.insert("model".into(), json!(model));
    out.insert("effort".into(), effort);
    out.insert("fast".into(), json!(i32::from(fast)));
    out.insert("usage_samples".into(), Value::Array(samples));
    out.insert("unpriced_models".into(), json!(unpriced));
    out
}
