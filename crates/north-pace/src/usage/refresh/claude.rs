//! Claude Code usage rows: `claude -p /usage` (0 turns, $0) refreshes ~/.claude.json's cachedUsageUtilization,
//! which this turns into one row per window, billing flag and unnamed bucket (contract: firn#12 plan §6).
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};

use super::record::Reading;
use crate::foundation::clock::Timestamp;
use crate::usage::{py, web};
use crate::usage::profiles;

const KNOWN: [&str; 6] = ["five_hour", "seven_day", "limits", "extra_usage", "spend", "cedar_ember"];
const FRESH_SECONDS: f64 = 60.0;
const REFRESH_LIMIT: Duration = Duration::from_secs(30);

pub(super) struct Settings {
    pub cfg: Value,
    pub host: String,
    pub state: PathBuf,
    pub home: PathBuf,
    pub configured: Option<PathBuf>,
    pub refresh: bool,
}

impl Settings {
    pub(super) fn from_env(refresh: bool) -> Self {
        let home = profiles::home();
        let state = std::env::var_os("XDG_STATE_HOME").filter(|path| !path.is_empty()).map_or_else(|| home.join(".local/state"), PathBuf::from);
        let configured = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|path| !path.is_empty()).map(PathBuf::from);
        Self { cfg: profiles::policy_value(&crate::pace::target::policy_path()), host: crate::platform::host::name(), state, home, configured, refresh }
    }
}

pub(super) fn read(settings: &Settings) -> Vec<Reading> {
    let now = py::second(Timestamp::now());
    let mut directories: Vec<Option<PathBuf>> = vec![None];
    directories.extend(profiles::profiles(&settings.cfg, &settings.host).into_iter().filter_map(|profile| profile.path).map(Some));
    directories.extend(settings.configured.clone().map(Some));
    directories.sort();
    directories.dedup();
    let mut rows = Vec::new();
    for directory in directories {
        if directory.as_ref().is_some_and(|directory| !directory.join(".credentials.json").exists()) {
            continue;
        }
        rows.extend(read_profile(settings, directory.as_deref(), &now));
    }
    rows
}

fn read_profile(settings: &Settings, directory: Option<&Path>, now: &str) -> Vec<Reading> {
    let path = directory.unwrap_or(&settings.home).join(".claude.json");
    let fetched = || std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()).and_then(|data| data["cachedUsageUtilization"]["fetchedAtMs"].as_f64()).unwrap_or(0.0) / 1000.0;
    if settings.refresh && Timestamp::now().unix_seconds_f64() - fetched() > FRESH_SECONDS {
        refresh(directory);
    }
    let Some(data) = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()) else { return Vec::new() };
    let mut cache = data["cachedUsageUtilization"].clone();
    if cache.as_object().is_some_and(|cache| !cache.is_empty()) && cache["accountUuid"].as_str().is_none_or(str::is_empty) {
        cache["accountUuid"] = json!(data["oauthAccount"]["accountUuid"].as_str().unwrap_or_default());
    }
    let mut rows = if cache.as_object().is_some_and(|cache| !cache.is_empty()) { rows(settings, &cache, now) } else { Vec::new() };
    let uuid = data["oauthAccount"]["accountUuid"].as_str().filter(|uuid| !uuid.is_empty()).or_else(|| cache["accountUuid"].as_str().filter(|uuid| !uuid.is_empty()));
    if let Some(account) = uuid.and_then(|uuid| profiles::label_of(&settings.cfg, uuid)) {
        rows.push(row(json!({"provider": "claude", "account": account, "kind": "billing", "name": "plan", "ts": now, "amount": 1, "source": profiles::observed_plan(&settings.home, directory)})));
    }
    rows
}

fn refresh(directory: Option<&Path>) {
    let mut command = Command::new("claude");
    command
        .args(["-p", "/usage", "--output-format", "json", "--setting-sources", "", "--strict-mcp-config", "--no-session-persistence"])
        .env("DISABLE_AUTOUPDATER", "1")
        .env("DISABLE_TELEMETRY", "1")
        .env("DISABLE_ERROR_REPORTING", "1")
        .env_remove("CLAUDE_CONFIG_DIR");
    if let Some(directory) = directory {
        command.env("CLAUDE_CONFIG_DIR", directory);
    }
    let Some(output) = crate::platform::process::run_command_within(command, None, REFRESH_LIMIT) else { return };
    if !output.status.success() {
        let errors = String::from_utf8_lossy(&output.stderr).to_lowercase();
        let throttled = errors.contains("429") || errors.contains("rate limit");
        eprintln!("usage_claude: refresh failed code={} throttled={throttled}", output.status.code().unwrap_or(-1));
    }
}

fn row(value: Value) -> Reading {
    const NULLABLE: [&str; 5] = ["used_pct", "window_min", "resets_at", "amount", "source"];
    let mut row = value.as_object().cloned().unwrap_or_default();
    for name in NULLABLE {
        row.entry(name).or_insert(Value::Null);
    }
    row
}

fn utc(text: &Value) -> Value {
    text.as_str().filter(|text| !text.is_empty()).and_then(Timestamp::parse_loose).map_or(Value::Null, |at| json!(py::second(at)))
}

fn rows(settings: &Settings, cache: &Value, now: &str) -> Vec<Reading> {
    let usage = &cache["utilization"];
    let uuid = cache["accountUuid"].as_str().unwrap_or_default();
    let hash = profiles::short_hash(uuid);
    let account = settings.cfg["accounts"]["claude"]["labels"][&hash].as_str().filter(|label| !label.is_empty()).map_or_else(|| hash.clone(), str::to_owned);
    let fetched = utc(&json!(Timestamp::from_unix_millis_f64(cache["fetchedAtMs"].as_f64().unwrap_or(0.0)).iso()));
    let ts = if fetched.is_null() { json!(now) } else { fetched };
    let base = |kind: &str, name: &str| json!({"provider": "claude", "account": account, "source": "claude -p /usage; ~/.claude.json cachedUsageUtilization", "ts": ts, "kind": kind, "name": name});
    let with = |mut base: Value, extra: Value| {
        for (key, value) in extra.as_object().into_iter().flatten() {
            base[key] = value.clone();
        }
        row(base)
    };
    let mut out = Vec::new();
    for limit in usage["limits"].as_array().into_iter().flatten() {
        let scope = limit["scope"]["model"]["display_name"].as_str().filter(|scope| !scope.is_empty());
        let name = format!("{}{}", limit["kind"].as_str().unwrap_or_default(), scope.map_or_else(String::new, |scope| format!("/{scope}")));
        let window = match limit["group"].as_str() {
            Some("session") => json!(300),
            Some("weekly") => json!(10080),
            _ => Value::Null,
        };
        out.push(with(base("window", &name), json!({"used_pct": limit["percent"], "window_min": window, "resets_at": utc(&limit["resets_at"])})));
    }
    let extra = &usage["extra_usage"];
    out.push(with(base("billing", "extra_usage"), json!({"used_pct": extra["utilization"], "amount": i32::from(extra["is_enabled"].as_bool().unwrap_or(false))})));
    if let Some(spend) = usage["spend"].as_object().filter(|spend| !spend.is_empty()) {
        let on = ["enabled", "auto_reload"].iter().any(|key| spend.get(*key).is_some_and(truthy));
        out.push(with(base("billing", "spend"), json!({"used_pct": spend.get("percent"), "amount": i32::from(on)})));
        if let Some(balance) = spend.get("balance").filter(|balance| !balance.is_null()) {
            out.push(with(base("billing", "credits"), json!({"amount": balance})));
        }
    }
    match usage["cedar_ember"]["grants"].as_array() {
        Some(grants) => {
            let left: i64 = grants.iter().map(|grant| grant["resets_left"].as_i64().unwrap_or(0)).sum();
            out.push(with(base("grant", "cedar_ember"), json!({"amount": left, "source": "~/.claude.json cachedUsageUtilization.cedar_ember"})));
        }
        None => {
            out.push(with(base("grant", "cedar_ember"), json!({})));
            out.extend(web_rows(settings, &hash, &account, &ts));
        }
    }
    for (name, bucket) in usage.as_object().into_iter().flatten() {
        if !KNOWN.contains(&name.as_str()) && bucket.as_object().is_some_and(|bucket| bucket.contains_key("utilization")) {
            out.push(with(base("grant", name), json!({"used_pct": bucket["utilization"], "resets_at": utc(&bucket["resets_at"])})));
        }
    }
    out
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(items) => !items.is_empty(),
        Value::Null => false,
    }
}

fn web_rows(settings: &Settings, key: &str, account: &str, ts: &Value) -> Vec<Reading> {
    let now = Timestamp::now();
    let Ok(Some(resets)) = web::resets_keyed(&settings.state, key, now) else { return Vec::new() };
    let mut grants = resets.grants;
    grants.sort_by_key(|grant| grant.name != "cedar_ember/full");
    grants
        .into_iter()
        .map(|grant| row(json!({"provider": "claude", "account": account, "kind": "grant", "name": grant.name, "resets_at": grant.expires, "amount": grant.amount as i64, "source": resets.source.clone(), "ts": ts})))
        .collect()
}
