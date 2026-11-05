use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;

const HEADERS: &str = "Messages response anthropic-ratelimit-unified headers on the account's setup token";
const HEADER_WINDOWS: [&str; 2] = ["5h", "7d"];

const FIVE_HOUR: (&str, u32) = ("session", 300);
const WEEK: (&str, u32) = ("weekly_all", 10080);
const SCOPED_PREFIX: &str = "weekly_scoped/";
const STREAM: &str = "claude stream rate_limit_event";

pub fn stream_rows(frame: &Value, at: f64) -> Vec<Value> {
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

pub fn window_of(key: &str) -> (String, Option<u32>) {
    match key {
        "five_hour" | "5h" => (FIVE_HOUR.0.to_owned(), Some(FIVE_HOUR.1)),
        "seven_day" | "7d" => (WEEK.0.to_owned(), Some(WEEK.1)),
        scoped => match scoped.strip_prefix("seven_day_") {
            Some(scope) if !scope.is_empty() => (format!("{SCOPED_PREFIX}{}", capitalised(scope)), Some(WEEK.1)),
            _ => (scoped.to_owned(), None),
        },
    }
}

fn capitalised(text: &str) -> String {
    let mut letters = text.chars();
    letters.next().map(|first| first.to_uppercase().chain(letters).collect()).unwrap_or_default()
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
