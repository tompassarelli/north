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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::{Places, table};
    use rusqlite::Connection;
    use std::fs;

    const HOST: &str = "fixture-host";

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    struct World {
        root: tempfile::TempDir,
    }

    impl World {
        fn new() -> Self {
            let world = Self { root: tempfile::tempdir().unwrap() };
            let source = world.home().join(".claude");
            for name in ["CLAUDE.md", "settings.json"] {
                write(&source.join(name), "shared harness");
            }
            for name in ["hooks", "skills", "agents", "commands"] {
                write(&source.join(name).join("fixture"), "shared harness");
            }
            let reset = "2099-01-05T00:00:00+00:00";
            for (directory, uuid, percent) in [(world.home(), "fixture-default", 98), (world.profile(), "fixture-c", 7)] {
                let accepted = directory == world.home();
                let fetched = Timestamp::now().unix_millis();
                write(
                    &directory.join(".claude.json"),
                    &json!({"oauthAccount": {"accountUuid": uuid},
                        "projects": {world.root.path().display().to_string(): {"hasTrustDialogAccepted": accepted, "allowedTools": [if accepted { "source-tool" } else { "profile-tool" }]},
                            "/project": {"hasTrustDialogAccepted": false}, "profile-only": {"hasTrustDialogAccepted": !accepted}},
                        "cachedUsageUtilization": {"accountUuid": uuid, "fetchedAtMs": fetched,
                            "utilization": {"limits": [{"kind": "weekly_all", "group": "weekly", "percent": percent, "resets_at": reset}]}}})
                    .to_string(),
                );
            }
            write(&world.profile().join(".credentials.json"), &json!({"claudeAiOauth": {"subscriptionType": "free", "rateLimitTier": "free"}}).to_string());
            write(&world.profile().join("settings.json"), "profile onboarding");
            world
        }

        fn home(&self) -> PathBuf {
            self.root.path().join("home")
        }

        fn profile(&self) -> PathBuf {
            self.root.path().join("profile")
        }

        fn cfg(&self) -> Value {
            let label = |uuid: &str| profiles::short_hash(uuid);
            let text = format!(
                "[accounts.claude]\nlabels = {{ \"{}\" = \"b\", \"{}\" = \"c\" }}\nplans = {{ c = \"max-5x\" }}\nauto_route_requires = {{ c = \"max-5x\" }}\nprofiles = {{ b = {{ \"{HOST}\" = {{ path = \"{}/.claude\" }} }}, c = {{ \"{HOST}\" = {{ path = \"{}\" }} }} }}\n",
                label("fixture-default"),
                label("fixture-c"),
                self.home().display(),
                self.profile().display()
            );
            crate::router::policy::json_value(text.parse::<toml_edit::DocumentMut>().unwrap().as_item())
        }

        fn settings(&self) -> Settings {
            Settings { cfg: self.cfg(), host: HOST.into(), state: self.root.path().join("state"), home: self.home(), configured: None, refresh: false }
        }
    }

    fn tidy(rows: &[Reading]) -> Vec<(String, String)> {
        let mut seen: Vec<_> = rows.iter().map(|row| (row["account"].as_str().unwrap().to_owned(), row["name"].as_str().unwrap().to_owned())).collect();
        seen.sort();
        seen
    }

    #[test]
    fn the_shared_harness_is_projected_into_each_profile_once_and_the_source_is_left_alone() {
        let world = World::new();
        let source_before = fs::read(world.home().join(".claude.json")).unwrap();
        let before: Value = serde_json::from_str(&fs::read_to_string(world.profile().join(".claude.json")).unwrap()).unwrap();
        let said = profiles::sync(&world.cfg(), HOST, &world.home()).unwrap();
        assert_eq!(said, [format!("Claude profile c: harness projected to {}", world.profile().display())]);
        let after = fs::read(world.profile().join(".claude.json")).unwrap();
        assert_eq!(profiles::sync(&world.cfg(), HOST, &world.home()).unwrap().len(), 1);
        assert_eq!(fs::read(world.profile().join(".claude.json")).unwrap(), after);
        let mut expected = before;
        expected["projects"][world.root.path().display().to_string()]["hasTrustDialogAccepted"] = json!(true);
        assert_eq!(serde_json::from_slice::<Value>(&after).unwrap(), expected);
        assert_eq!(fs::read(world.home().join(".claude.json")).unwrap(), source_before);
        assert_eq!(fs::read_to_string(world.profile().join(".credentials.json")).unwrap(), json!({"claudeAiOauth": {"subscriptionType": "free", "rateLimitTier": "free"}}).to_string());
        assert_eq!(fs::read_to_string(world.profile().join("settings.json.before-harness-sync")).unwrap(), "profile onboarding");
        for name in profiles::PROJECTED {
            let link = world.profile().join(name);
            assert!(link.is_symlink() && fs::canonicalize(&link).unwrap() == fs::canonicalize(world.home().join(".claude").join(name)).unwrap(), "{name}");
        }
    }

    #[test]
    fn a_second_backup_of_the_same_file_is_refused() {
        let world = World::new();
        write(&world.profile().join("CLAUDE.md"), "profile");
        write(&world.profile().join("CLAUDE.md.before-harness-sync"), "older");
        let error = profiles::sync(&world.cfg(), HOST, &world.home()).unwrap_err();
        assert!(error.starts_with("profile projection backup already exists"), "{error}");
    }

    #[test]
    fn every_signed_in_profile_reports_its_own_account_and_plan() {
        let world = World::new();
        let rows = read(&world.settings());
        let accounts: std::collections::BTreeSet<_> = rows.iter().map(|row| row["account"].as_str().unwrap().to_owned()).collect();
        assert_eq!(accounts, ["b".to_owned(), "c".to_owned()].into_iter().collect());
        let plan = |rows: &[Reading]| rows.iter().find(|row| row["name"] == "plan" && row["account"] == "c").map(|row| row["source"].clone());
        assert_eq!(plan(&rows), Some(json!("free")));
        write(&world.profile().join(".credentials.json"), &json!({"claudeAiOauth": {"subscriptionType": "max", "rateLimitTier": "default_claude_max_5x"}}).to_string());
        let upgraded = read(&world.settings());
        assert_eq!(plan(&upgraded), Some(json!("max-5x")));
        let db = Connection::open_in_memory().unwrap();
        super::super::record::record(&db, &upgraded).unwrap();
        let places = Places { policy: world.root.path().join("orchestration.toml"), operator: world.root.path().join("op"), state: world.root.path().join("state") };
        fs::write(&places.policy, "[accounts.claude]\nplans = { c = \"max-5x\" }\n[accounts.claude.profiles.b]\nfixture-host = {}\n[accounts.claude.profiles.c]\nfixture-host = {}\n").unwrap();
        let (text, _) = table::render(&db, &places, Timestamp::now()).unwrap();
        let line = text.lines().find(|line| line.starts_with("claude c ")).unwrap();
        assert!(line.contains("weekly_all 7%") && line.contains("plan Claude Max 5x $100/month"), "{line}");
    }

    #[test]
    fn an_upgraded_account_reports_the_plan_of_its_refreshed_organization_tier_not_its_sign_in_credential() {
        let world = World::new();
        write(&world.profile().join(".claude.json"), &json!({"oauthAccount": {"accountUuid": "fixture-c", "organizationRateLimitTier": "default_claude_max_20x"}}).to_string());
        write(&world.profile().join(".credentials.json"), &json!({"claudeAiOauth": {"subscriptionType": "max", "rateLimitTier": "default_claude_max_5x"}}).to_string());
        let rows = read(&world.settings());
        let plan = rows.iter().find(|row| row["name"] == "plan" && row["account"] == "c").unwrap();
        assert_eq!((plan["account"].as_str(), plan["source"].as_str()), (Some("c"), Some("max-20x")));
    }

    #[test]
    fn a_profile_without_credentials_is_not_read_and_the_default_profile_always_is() {
        let world = World::new();
        fs::remove_file(world.profile().join(".credentials.json")).unwrap();
        assert_eq!(tidy(&read(&world.settings())).iter().map(|(account, _)| account.as_str()).collect::<std::collections::BTreeSet<_>>(), ["b"].into_iter().collect());
    }

    #[test]
    fn windows_billing_and_unnamed_buckets_become_rows_and_reset_availability_comes_from_the_web_cache() {
        let world = World::new();
        let cache = json!({"accountUuid": "fixture-default", "fetchedAtMs": 1_790_000_000_000_i64, "utilization": {
            "limits": [{"kind": "five_hour", "group": "session", "percent": 12.5, "resets_at": "2026-11-04T16:00:00+00:00"},
                {"kind": "weekly_scoped", "group": "weekly", "percent": 3, "scope": {"model": {"display_name": "Fable"}}, "resets_at": null}],
            "extra_usage": {"is_enabled": false, "utilization": 0},
            "spend": {"percent": 1.5, "enabled": false, "auto_reload": true, "balance": 4.25},
            "cedar_ember": {},
            "mystery": {"utilization": 9, "resets_at": "2026-11-05T00:00:00Z"}}});
        write(&world.root.path().join("state/agents/usage/claude-web.json"), &json!({"accounts": {profiles::short_hash("fixture-default"): {"account_label": "b", "read_at": "2026-11-04T14:06:00Z",
            "resets": {"full": {"count": 2, "expires": "2099-11-01T00:00:00Z"}, "five_hour": {"count": 3}}}}}).to_string());
        let rows = rows(&world.settings(), &cache, "2026-11-04T15:00:00Z");
        let shown: Vec<_> = rows.iter().map(|row| (row["kind"].as_str().unwrap(), row["name"].as_str().unwrap(), row["amount"].clone(), row["used_pct"].clone(), row["window_min"].clone())).collect();
        assert_eq!(
            shown,
            [
                ("window", "five_hour", Value::Null, json!(12.5), json!(300)),
                ("window", "weekly_scoped/Fable", Value::Null, json!(3), json!(10080)),
                ("billing", "extra_usage", json!(0), json!(0), Value::Null),
                ("billing", "spend", json!(1), json!(1.5), Value::Null),
                ("billing", "credits", json!(4.25), Value::Null, Value::Null),
                ("grant", "cedar_ember", Value::Null, Value::Null, Value::Null),
                ("grant", "cedar_ember/full", json!(2), Value::Null, Value::Null),
                ("grant", "cedar_ember/five_hour", json!(3), Value::Null, Value::Null),
                ("grant", "mystery", Value::Null, json!(9), Value::Null),
            ]
        );
        assert_eq!(rows[0]["account"], "b");
        assert_eq!(rows[0]["resets_at"], "2026-11-04T16:00:00Z");
        assert_eq!(rows[6]["resets_at"], "2099-11-01T00:00:00Z");
    }

    #[test]
    fn the_rows_equal_the_ones_the_python_adapter_printed_on_the_same_recorded_profiles() {
        let recorded: Value = serde_json::from_str(include_str!("../fixtures/claude_python.json")).unwrap();
        let root = tempfile::tempdir().unwrap();
        for (relative, text) in recorded["files"].as_object().unwrap() {
            write(&root.path().join(relative), text.as_str().unwrap());
        }
        let policy = recorded["policy"].as_str().unwrap().replace("{HOST}", HOST).replace("{ROOT}", &root.path().display().to_string());
        let cfg = crate::router::policy::json_value(policy.parse::<toml_edit::DocumentMut>().unwrap().as_item());
        let settings = Settings { cfg, host: HOST.into(), state: root.path().join("state"), home: root.path().join("home"), configured: None, refresh: false };
        let normal = |row: &Value| {
            let cells: Vec<Value> = crate::usage::store::COLUMNS.iter().map(|column| if *column == "ts" && row["name"] == "plan" { Value::Null } else { row.get(*column).cloned().unwrap_or(Value::Null) }).collect();
            Value::Array(cells)
        };
        let key = |row: &Value| row.to_string();
        let mut actual: Vec<Value> = read(&settings).iter().map(|row| normal(&Value::Object(row.clone()))).collect();
        let mut expected: Vec<Value> = recorded["rows"].as_array().unwrap().iter().map(normal).collect();
        actual.sort_by_key(key);
        expected.sort_by_key(key);
        assert_eq!(actual.len(), 20);
        assert_eq!(actual, expected);
    }
}
