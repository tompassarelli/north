use serde_json::{Value, json};

use super::meter;
use crate::foundation::clock::Timestamp;
use crate::foundation::limit_clock::{limit_reply, reset_after, reset_on};

const HEADERS: &str = "Messages response anthropic-ratelimit-unified headers on the account's setup token";
const HEADER_WINDOWS: [&str; 2] = ["5h", "7d"];

const FIVE_HOUR: (&str, u32) = ("session", 300);
const WEEK: (&str, u32) = ("weekly_all", 10080);
const STREAM: &str = "claude stream rate_limit_event";
const REPLY: &str = "claude stream usage-limit reply";
const UNNAMED_RESET_S: i64 = 3600;

pub fn stream_rows(frame: &Value, at: f64) -> Vec<Value> {
    if frame["type"] == "assistant" && frame["parent_tool_use_id"].is_null() {
        return frame["message"]["content"][0]["text"].as_str().map(|text| reply_rows(text, at)).unwrap_or_default();
    }
    if frame["type"] != "rate_limit_event" {
        return Vec::new();
    }
    let info = &frame["rate_limit_info"];
    let mut rows = Vec::new();
    for (key, window) in info["unifiedWindows"].as_object().into_iter().flatten() {
        let Some(used) = window["utilization"].as_f64() else { continue };
        let (name, minutes) = window_of(key);
        rows.push(window_row(None, &name, minutes, used, window["resetsAt"].as_i64(), STREAM, at));
    }
    let Some(kind) = info["rateLimitType"].as_str() else { return rows };
    let (name, _) = window_of(kind);
    if let Some(status) = info["status"].as_str() {
        rows.push(status_row(None, &name, status, STREAM, at));
        if status == "rejected" {
            let resets = info["resetsAt"].as_i64().or_else(|| info["unifiedWindows"][kind]["resetsAt"].as_i64());
            rows.push(limit_row(None, &name, resets, STREAM, at));
        }
    }
    rows
}

/// The full window a model's own `You've hit your Fable limit · resets Nov 19, 9am` reply names, which the provider sends as an ordinary assistant turn, with its limit row.
fn reply_rows(text: &str, at: f64) -> Vec<Value> {
    let Some(named) = limit_reply(text) else { return Vec::new() };
    let (name, minutes) = match meter::family(&named) {
        Some(family) => (meter::scoped(family), Some(WEEK.1)),
        None if named.contains("week") => (WEEK.0.to_owned(), Some(WEEK.1)),
        None => (FIVE_HOUR.0.to_owned(), Some(FIVE_HOUR.1)),
    };
    let now_ms = (at * 1000.0) as u64;
    let resets = reset_on(text, now_ms).or_else(|| reset_after(text, now_ms)).map_or(at as i64 + UNNAMED_RESET_S, |ms| (ms / 1000) as i64);
    vec![window_row(None, &name, minutes, 1.0, Some(resets), REPLY, at), limit_row(None, &name, Some(resets), REPLY, at)]
}

pub fn window_of(key: &str) -> (String, Option<u32>) {
    match key {
        "five_hour" | "5h" => (FIVE_HOUR.0.to_owned(), Some(FIVE_HOUR.1)),
        "seven_day" | "7d" => (WEEK.0.to_owned(), Some(WEEK.1)),
        scoped => match scoped.strip_prefix("seven_day_") {
            Some(scope) if !scope.is_empty() => (meter::family(scope).map_or_else(|| format!("weekly_scoped/{scope}"), meter::scoped), Some(WEEK.1)),
            _ => (scoped.to_owned(), None),
        },
    }
}

pub fn window_row(account: Option<&str>, name: &str, minutes: Option<u32>, used: f64, resets_at: Option<i64>, source: &str, at: f64) -> Value {
    json!({"provider": "claude", "account": account, "source": source, "ts": iso(at), "kind": "window", "name": name,
        "used_pct": (used * 100.0).round(), "window_min": minutes, "resets_at": resets_at.map(|seconds| iso(seconds as f64)), "amount": null})
}

pub fn status_row(account: Option<&str>, name: &str, status: &str, source: &str, at: f64) -> Value {
    json!({"provider": "claude", "account": account, "source": format!("{source}; status={status}"), "ts": iso(at), "kind": "status", "name": name,
        "used_pct": null, "window_min": null, "resets_at": null, "amount": null})
}

pub fn limit_row(account: Option<&str>, name: &str, resets_at: Option<i64>, source: &str, at: f64) -> Value {
    json!({"provider": "claude", "account": account, "source": format!("{source}; rejected"), "ts": iso(at), "kind": "limit", "name": name,
        "used_pct": 100.0, "window_min": null, "resets_at": resets_at.map(|seconds| iso(seconds as f64)), "amount": null})
}

fn iso(seconds: f64) -> String {
    Timestamp::from_unix_seconds_f64(seconds).iso()
}

pub fn header_rows(account: &str, headers: &str, at: f64) -> Result<Vec<Value>, String> {
    let refused = headers.lines().next().is_some_and(|line| line.split_whitespace().nth(1) == Some("429"));
    let claim = header(headers, "representative-claim").map(|claim| window_of(claim).0);
    let mut rows = Vec::new();
    for key in HEADER_WINDOWS {
        let (name, minutes) = window_of(key);
        let reset = header(headers, &format!("{key}-reset")).and_then(|seconds| seconds.parse::<f64>().ok()).map(|seconds| seconds as i64);
        let status = header(headers, &format!("{key}-status"));
        if let Some(used) = header(headers, &format!("{key}-utilization")).and_then(|used| used.parse::<f64>().ok()) {
            rows.push(window_row(Some(account), &name, minutes, used, reset, HEADERS, at));
        }
        if let Some(status) = status {
            rows.push(status_row(Some(account), &name, status, HEADERS, at));
        }
        if status == Some("rejected") || (refused && reset.is_some() && claim.as_deref() == Some(name.as_str())) {
            rows.push(limit_row(Some(account), &name, reset, HEADERS, at));
        }
    }
    if rows.iter().any(|row| row["kind"] == "window") { Ok(rows) } else { Err("response carried no anthropic-ratelimit-unified utilization headers".into()) }
}

fn header<'a>(headers: &'a str, name: &str) -> Option<&'a str> {
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim().eq_ignore_ascii_case(&format!("anthropic-ratelimit-unified-{name}")).then(|| value.trim())
    })
}
