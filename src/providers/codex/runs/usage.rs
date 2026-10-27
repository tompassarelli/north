use std::collections::{BTreeSet, HashMap};

use serde_json::{Map, Value, json};

use crate::providers::claude::runs::int;
use crate::providers::claude::runs::telemetry::{Prices, Request, hook_fire, idle_gap_misses};

fn key(payload: &Value, cumulative: &Value, last: &Value) -> String {
    match &payload["request_id"] {
        Value::String(id) if !id.is_empty() => id.clone(),
        _ => json!([cumulative, last]).to_string(),
    }
}

pub(super) fn context_metrics(records: &[Value]) -> Map<String, Value> {
    let (mut boundaries, mut summaries, mut handoffs) = (0, 0, 0);
    let mut order: Vec<String> = Vec::new();
    let mut requests: HashMap<String, Request> = HashMap::new();
    for record in records {
        let kind = record["type"].as_str().unwrap_or("");
        let payload = &record["payload"];
        let inner = payload["type"].as_str().unwrap_or("");
        boundaries += i64::from(kind == "compacted");
        summaries += i64::from(kind == "event_msg" && inner == "context_compacted");
        if kind == "response_item" && inner == "message" && matches!(payload["role"].as_str(), Some("user" | "developer")) {
            handoffs += i64::from(hook_fire(record));
        }
        if kind == "event_msg" && inner == "user_message" {
            let text = payload["message"].as_str().unwrap_or("");
            handoffs += i64::from(hook_fire(&json!({"content": text})));
        }
        let info = &payload["info"];
        if kind != "event_msg" || inner != "token_count" || !crate::providers::claude::runs::truthy(info) {
            continue;
        }
        let usage = &info["last_token_usage"];
        let cache = int(&usage["cached_input_tokens"]);
        let total = int(&usage["input_tokens"]);
        let id = key(payload, &info["total_token_usage"], usage);
        let at = record["timestamp"].as_str().map(str::to_owned);
        match requests.get_mut(&id) {
            None => {
                order.push(id.clone());
                requests.insert(id, Request { at, total, cache });
            }
            Some(previous) if total > previous.total => {
                previous.total = total;
                previous.cache = cache;
            }
            Some(_) => {}
        }
    }
    let mut values: Vec<&Request> = order.iter().map(|id| &requests[id]).collect();
    values.sort_by(|left, right| left.at.as_deref().unwrap_or("").cmp(right.at.as_deref().unwrap_or("")));
    let mut out = Map::new();
    out.insert("peak_ctx".into(), json!(values.iter().map(|value| value.total).max().unwrap_or(0)));
    out.insert("compactions".into(), json!(boundaries.max(summaries)));
    out.insert("handoff_fires".into(), json!(handoffs));
    out.insert("handoff_ignored".into(), Value::Null);
    out.insert("input_tokens".into(), json!(values.iter().map(|value| value.total).sum::<i64>()));
    out.insert("cache_read_tokens".into(), json!(values.iter().map(|value| value.cache).sum::<i64>()));
    out.insert("idle_gap_misses".into(), json!(idle_gap_misses(&values)));
    out
}

fn fast(tier: &Value) -> Option<bool> {
    tier.as_str().map(|tier| matches!(tier, "fast" | "priority"))
}

fn delta(cumulative: &Value, last: &Value, previous: &Value) -> HashMap<String, i64> {
    let mut usage: HashMap<String, i64> = HashMap::new();
    for (name, value) in cumulative.as_object().into_iter().flatten() {
        usage.insert(name.clone(), (int(value) - int(&previous[name.as_str()])).max(0));
    }
    for (name, value) in last.as_object().into_iter().flatten() {
        if cumulative.get(name).is_none() {
            usage.insert(name.clone(), int(value));
        }
    }
    usage
}

pub(super) fn run_usage(records: &[Value], prices: &Prices) -> Map<String, Value> {
    let (mut model, mut effort, mut fast_tier): (Option<String>, Option<String>, bool) = (None, None, false);
    let mut samples: Vec<Value> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut unpriced = BTreeSet::new();
    let mut previous = Value::Null;
    for record in records {
        let payload = &record["payload"];
        if record["type"] == "session_meta"
            && let Some(flag) = fast(&payload["service_tier"])
        {
            fast_tier = flag;
        }
        if record["type"] == "turn_context" {
            model = payload["model"].as_str().filter(|name| !name.is_empty()).map(str::to_owned).or(model);
            effort = payload["effort"].as_str().filter(|name| !name.is_empty()).map(str::to_owned).or(effort);
            if let Some(flag) = fast(&payload["service_tier"]) {
                fast_tier = flag;
            }
        }
        if record["type"] != "event_msg" || payload["type"] != "token_count" || !crate::providers::claude::runs::truthy(&payload["info"]) {
            continue;
        }
        let info = &payload["info"];
        let (cumulative, last) = (&info["total_token_usage"], &info["last_token_usage"]);
        let id = key(payload, cumulative, last);
        if seen.contains_key(&id) {
            continue;
        }
        let usage = delta(cumulative, last, &previous);
        previous = cumulative.clone();
        let read = usage.get("cached_input_tokens").copied().unwrap_or(0);
        let input = (usage.get("input_tokens").copied().unwrap_or(0) - read).max(0);
        let output = usage.get("output_tokens").copied().unwrap_or(0);
        let Some(at) = record["timestamp"].as_str().filter(|at| !at.is_empty()) else { continue };
        let (weight, priced) = prices.weight(model.as_deref(), [input as f64, 0.0, read as f64, output as f64]);
        if weight != 0.0 && !priced && prices.by_price() {
            unpriced.insert(model.clone().unwrap_or_else(|| "unknown".into()));
        }
        seen.insert(id.clone(), samples.len());
        samples.push(json!({"request": id, "ts": at, "weight": weight}));
    }
    if effort.is_none() {
        effort = prices.effort(model.as_deref()).as_str().map(str::to_owned);
    }
    let mut out = Map::new();
    out.insert("account".into(), json!("codex"));
    out.insert("model".into(), json!(model));
    out.insert("effort".into(), json!(effort));
    out.insert("fast".into(), json!(i32::from(fast_tier)));
    out.insert("usage_samples".into(), Value::Array(samples));
    out.insert("unpriced_models".into(), json!(unpriced));
    out
}
