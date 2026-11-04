use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use super::store::{self, Source};
use super::{Action, Places, costs, report, table};
use crate::foundation::clock::Timestamp;

const STATES: &str = include_str!("fixtures/states.json");
const CREATE: &str = "CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT";
const FROZEN: &str = "2026-10-28T12:00:00Z";

struct Scratch {
    dir: tempfile::TempDir,
}

impl Scratch {
    fn new(state: &Value) -> Self {
        let scratch = Self { dir: tempfile::tempdir().unwrap() };
        ledger(&scratch.db(), &state["rows"]);
        fs::write(scratch.places().policy, state["orchestration"].as_str().unwrap_or_default()).unwrap();
        if !state["web"].is_null() {
            fs::create_dir_all(scratch.places().state.join("agents/usage")).unwrap();
            fs::write(scratch.places().state.join("agents/usage/claude-web.json"), state["web"].to_string()).unwrap();
        }
        scratch
    }

    fn db(&self) -> PathBuf {
        self.dir.path().join("threads.db")
    }

    fn places(&self) -> Places {
        let root = self.dir.path();
        Places { policy: root.join("orchestration.toml"), operator: root.join("op"), state: root.join("state") }
    }

    fn open(&self) -> Connection {
        store::open(&Source::locate(self.dir.path(), Some(self.db().into())), &|| {}).unwrap()
    }

    fn json(&self) -> String {
        report(&self.open(), &self.places().policy, frozen()).unwrap() + "\n"
    }

    fn text(&self) -> String {
        table::render(&self.open(), &self.places(), frozen()).unwrap().0 + "\n"
    }
}

fn frozen() -> Timestamp {
    Timestamp::parse_rfc3339(FROZEN).unwrap()
}

fn states() -> Vec<Value> {
    serde_json::from_str(STATES).unwrap()
}

fn ledger(path: &Path, rows: &Value) {
    let db = Connection::open(path).unwrap();
    db.execute(CREATE, []).unwrap();
    for row in rows.as_array().unwrap() {
        let values: Vec<rusqlite::types::Value> = row
            .as_array()
            .unwrap()
            .iter()
            .map(|cell| match cell {
                Value::Null => rusqlite::types::Value::Null,
                Value::String(text) => rusqlite::types::Value::Text(text.clone()),
                number if number.is_i64() => rusqlite::types::Value::Integer(number.as_i64().unwrap()),
                number => rusqlite::types::Value::Real(number.as_f64().unwrap()),
            })
            .collect();
        db.execute("INSERT INTO usage VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)", rusqlite::params_from_iter(values)).unwrap();
    }
}

#[test]
fn the_table_names_burn_resets_plan_and_billing_of_each_account() {
    let table = Scratch::new(&states()[3]).text();
    for part in [
        "burn 5.0%/h, empty in 10h00m (28 Oct 22:00Z; 3 readings)",
        "resets full=2 (expires 2026-11-01T00:00:00Z) 5h=3 (claude.ai settings/usage via Chrome; until 29 Oct 00:00Z)",
        "resets=1 (web: usage page)",
        "resets=unknown (no web reset reading for this account)",
        "resets=unknown (provider adapter has no reset availability)",
        "burn unknown (1 reading)",
        "lasts to reset",
        "| plan Claude Max 20x $200/month",
        "| plan Claude Max 5x $100/month",
        "credit=1.23457e+06, tiny=1e-05",
        ", STALE)",
        "extra_usage=0 |",
        "session 12% used",
        "extra 0% used, resets in -",
    ] {
        assert!(table.contains(part), "{part}\n{table}");
    }
}

#[test]
fn a_stale_flag_marks_readings_older_than_the_stale_minutes() {
    let scratch = Scratch::new(&states()[1]);
    let rows: Vec<Value> = serde_json::from_str(&scratch.json()).unwrap();
    let stale = |provider: &str| rows.iter().find(|row| row["provider"] == provider).map(|row| row["stale"].clone());
    assert_eq!((stale("claude"), stale("gemini")), (Some(true.into()), Some(false.into())));
}

#[test]
fn a_ledger_without_a_usage_table_or_file_prints_no_readings_and_exits_1() {
    let scratch = Scratch::new(&serde_json::json!({"rows": [], "orchestration": ""}));
    let bare = scratch.dir.path().join("bare.db");
    Connection::open(&bare).unwrap().execute("CREATE TABLE other(x)", []).unwrap();
    for db in [bare, scratch.dir.path().join("absent.db")] {
        let source = Source::locate(scratch.dir.path(), Some(db.clone().into()));
        let open = || store::open(&source, &|| {}).unwrap();
        let (json, (text, code)) = (report(&open(), &scratch.places().policy, frozen()).unwrap(), table::render(&open(), &scratch.places(), frozen()).unwrap());
        assert_eq!((json.as_str(), code), ("[]", 1), "{}", db.display());
        assert!(text.starts_with("agents usage: no snapshot yet"), "{text}");
    }
}

#[test]
fn a_remote_host_reads_the_mirrored_snapshot_as_the_ledger_host_reads_its_own() {
    let state = &states()[0];
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".config/threads")).unwrap();
    fs::write(home.path().join(".config/threads/remote"), "far\n").unwrap();
    fs::create_dir_all(home.path().join(".local/state/threads")).unwrap();
    ledger(&home.path().join(".local/state/threads/snapshot.db"), &state["rows"]);
    let places = Places { policy: home.path().join("orchestration.toml"), operator: home.path().join("op"), state: home.path().join("state") };
    fs::write(&places.policy, state["orchestration"].as_str().unwrap()).unwrap();
    let db = store::open(&Source::locate(home.path(), None), &|| {}).unwrap();
    let actual = report(&db, &places.policy, frozen()).unwrap() + "\n";
    assert_eq!(actual, Scratch::new(state).json());
    assert!(actual.len() > 3);
}

const COSTS: &str = r#"[[service]]
id = "claude-a"
kind = "ai-plan"
billing = "fixed"
usd_per_week = 50.0
use = "dev"
projects = {}
readout = "agents usage"

[[service]]
id = "cloud-credit"
kind = "ai-plan"
billing = "prepaid"
usd_per_week = 0.0
initial = 250.0
expires = 2099-11-05T07:59:00Z
use = "dev"
projects = {}
readout = "manual"

[[service]]
id = "digitalocean"
kind = "cloud"
billing = "metered"
usd_per_week = 0.0
use = "dev"
projects = {}
readout = "x"

[[service]]
id = "hetzner-gex45"
kind = "cloud"
billing = "fixed"
usd_per_week = 40.0
use = "dev"
projects = {}
readout = "x"

[[service]]
id = "rental"
kind = "gpu-rental"
billing = "prepaid"
usd_per_week = 0.0
use = "dev"
projects = {}
readout = "x"
"#;

fn cost_places(root: &Path) -> Places {
    let places = Places { policy: root.join("orchestration.toml"), operator: root.join("op"), state: root.join("state") };
    fs::create_dir_all(&places.operator).unwrap();
    fs::create_dir_all(places.state.join("agents/costs")).unwrap();
    fs::write(places.operator.join("costs.toml"), COSTS).unwrap();
    let readout = |id: &str, body: &str| fs::write(places.state.join("agents/costs").join(format!("{id}.json")), body).unwrap();
    readout("rental", r#"{"service":"rental","period_start":"2026-10-01T00:00:00Z","period_end":"2026-10-15T00:00:00Z","usd_spent_period":20.0,"usd_balance":14.5,"credits_left":null,"read_at":"2026-10-12T05:00:00Z","source":"invoices"}"#);
    readout("digitalocean", r#"{"service":"digitalocean","period_start":"2026-10-01T00:00:00+00:00","period_end":"2026-11-01T00:00:00+00:00","usd_spent_period":18.33,"usd_balance":18.33,"credits_left":0.0,"read_at":"2026-10-12T07:12:00+00:00","source":"do"}"#);
    readout("cloud-credit", r#"{"usd_spent_period":20.0}"#);
    places
}

#[test]
fn agents_usage_prints_one_cost_line_per_non_ai_service_and_the_weekly_operating_total() {
    let root = tempfile::tempdir().unwrap();
    let places = cost_places(root.path());
    let now = Timestamp::parse_rfc3339("2026-10-12T08:12:00Z").unwrap();
    let want = "cloud-credit (ai-plan, prepaid): $230.00 left of $250.00, expires 2099-11-05T07:59Z (readout: manual)\n\
digitalocean (cloud, metered): $18.33 spent 2026-10-01..2026-11-01, balance $18.33, credits left 0 (do, 60 min ago)\n\
hetzner-gex45 (cloud, fixed): no readout yet\n\
rental (gpu-rental, prepaid): $20.00 spent 2026-10-01..2026-10-15, balance $14.50 (invoices, 192 min ago)\n\
weekly operating $113.85 = fixed $90.00 + metered $23.85";
    assert_eq!(costs::lines(&places, now).unwrap().join("\n"), want);
    let empty = Connection::open_in_memory().unwrap();
    assert_eq!(table::render(&empty, &places, now).unwrap(), (format!("agents usage: no snapshot yet; run `agents usage --refresh`\n{want}"), 1));
    fs::remove_file(places.state.join("agents/costs/cloud-credit.json")).unwrap();
    assert_eq!(
        costs::lines(&places, now).unwrap()[0],
        "cloud-credit (ai-plan, prepaid): no readout, unknown left of $250.00, expires 2099-11-05T07:59Z (readout: manual)"
    );
}

#[test]
fn a_retired_providers_stored_readings_are_not_shown_by_the_json_or_the_table() {
    let rows = |tuple: [(&str, &str, f64); 3]| {
        Value::Array(tuple.iter().map(|(ts, provider, used)| serde_json::json!([ts, provider, "main", "window", "week", used, 10080, "2099-01-01T00:00:00Z", null, "fixture"])).collect())
    };
    let scratch = Scratch::new(&serde_json::json!({
        "rows": rows([("2026-10-27T10:00:00Z", "codex", 40.0), ("2026-10-27T10:00:00Z", "gemini", 10.0), ("2026-10-27T10:30:00Z", "gemini", 11.0)]),
        "orchestration": ""
    }));
    let shown: Vec<Value> = serde_json::from_str(&scratch.json()).unwrap();
    assert_eq!(shown.iter().map(|row| row["provider"].as_str().unwrap()).collect::<Vec<_>>(), ["gemini"]);
    let text = scratch.text();
    assert!(!text.contains("codex") && text.contains("gemini main") && text.contains("2 readings"), "{text}");
}

#[test]
fn a_corrupt_snapshot_is_refreshed_once_and_persistent_corruption_is_reported_after_that_one_retry() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".config/threads")).unwrap();
    fs::write(home.path().join(".config/threads/remote"), "far\n").unwrap();
    fs::create_dir_all(home.path().join(".local/state/threads")).unwrap();
    let snapshot = home.path().join(".local/state/threads/snapshot.db");
    let good = home.path().join("canonical.db");
    ledger(&good, &states()[0]["rows"]);
    let corrupt = || fs::write(&snapshot, [b"SQLite format 3\0".as_slice(), &[b'x'; 600]].concat()).unwrap();
    let source = Source::locate(home.path(), None);

    corrupt();
    let pulls = Cell::new(0);
    let repaired = store::open(&source, &|| {
        pulls.set(pulls.get() + 1);
        fs::copy(&good, &snapshot).unwrap();
    })
    .unwrap();
    assert_eq!(pulls.get(), 1);
    assert!(!store::latest(&repaired).unwrap().is_empty());

    corrupt();
    let before = fs::read(&snapshot).unwrap();
    pulls.set(0);
    let error = store::open(&source, &|| pulls.set(pulls.get() + 1)).unwrap_err();
    assert!(error.starts_with("NorthThreads: snapshot still unreadable after one refresh"), "{error}");
    assert_eq!((pulls.get(), fs::read(&snapshot).unwrap()), (1, before));
}

const CTX_OLD: &str = include_str!("fixtures/ctx_python.json");

fn same(left: &Value, right: &Value, path: &str) {
    match (left, right) {
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>(), "{path}");
            a.iter().for_each(|(key, value)| same(value, &b[key], &format!("{path}.{key}")));
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            a.iter().zip(b).enumerate().for_each(|(index, (x, y))| same(x, y, &format!("{path}[{index}]")));
        }
        (Value::Number(a), Value::Number(b)) if a.is_f64() && b.is_f64() => {
            assert!((a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-6, "{path}: {a} vs {b}");
        }
        _ => assert_eq!(left, right, "{path}"),
    }
}

#[test]
fn the_ctx_built_for_the_router_equals_the_one_the_python_plan_usage_built_on_a_recorded_ledger_with_runs_and_model_events() {
    let state: Value = serde_json::from_str(include_str!("fixtures/gate_state.json")).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let (db_path, policy) = (scratch.path().join("threads.db"), scratch.path().join("orchestration.toml"));
    ledger(&db_path, &state["rows"]);
    let db = Connection::open(&db_path).unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, tier TEXT, category TEXT, outcome TEXT, expects_landing INTEGER, landed INTEGER, item TEXT, role TEXT, follows TEXT); CREATE TABLE model_events(model TEXT, at TEXT, p REAL, kind TEXT, ts TEXT);").unwrap();
    for (sql, rows) in [("INSERT INTO runs VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)", &state["runs"]), ("INSERT INTO model_events VALUES (?1, ?2, ?3, ?4, ?5)", &state["events"])] {
        for row in rows.as_array().unwrap() {
            let cells = row.as_array().unwrap().iter().map(|cell| match cell {
                Value::Null => rusqlite::types::Value::Null,
                Value::String(text) => rusqlite::types::Value::Text(text.clone()),
                number if number.is_i64() => rusqlite::types::Value::Integer(number.as_i64().unwrap()),
                number => rusqlite::types::Value::Real(number.as_f64().unwrap()),
            });
            db.execute(sql, rusqlite::params_from_iter(cells)).unwrap();
        }
    }
    fs::write(&policy, include_str!("../router/fixtures/policy/orchestration.toml")).unwrap();
    let expected: Value = serde_json::from_str(CTX_OLD).unwrap();
    let inputs = super::gate::Inputs::load(&policy).unwrap();
    let readings = super::readings(&db, &policy).unwrap();
    for provider in ["claude", "gemini"] {
        let mut actual = inputs.context(&db, &readings, provider, frozen()).unwrap();
        actual.as_object_mut().unwrap().remove("now");
        same(&expected[provider], &actual, provider);
    }
    assert_eq!(expected["claude"]["success"], serde_json::json!({"claude:haiku|tooling": 60}));
    assert_eq!(expected["claude"]["tiers"][0]["successor"]["model"], "claude-haiku-5-7");
}

#[test]
fn arguments_only_the_python_verbs_serve_are_left_to_them() {
    let handled = |text: &str| Action::parse(&text.split_whitespace().map(str::to_owned).collect::<Vec<_>>()).is_some();
    assert!(handled("--refresh --local --json") && handled("--fast claude --cached --domain lead") && handled("--write-pace-gate --fast claude"));
    assert!(!handled("--fast") && !handled("--gate claude"));
}

#[test]
fn one_window_a_refresh_did_not_reread_cannot_hold_a_freshly_read_account_stale_at_the_spawn_gate() {
    let refusal = |fable_ts: &str, fresh_ts: &str| {
        let rows = serde_json::json!([
            [fresh_ts, "claude", "a", "window", "session", 20.0, 300, "2026-10-28T14:00:00Z", null, "headers"],
            [fresh_ts, "claude", "a", "window", "weekly_all", 10.0, 10080, "2026-11-03T00:00:00Z", null, "headers"],
            [fable_ts, "claude", "a", "window", "weekly_scoped/Fable", 0.0, 10080, "2026-11-03T00:00:00Z", null, "web"],
            [fable_ts, "claude", "a", "billing", "extra_usage", null, null, null, 0.0, "web"],
        ]);
        let scratch = Scratch::new(&serde_json::json!({"rows": rows, "orchestration": include_str!("../router/fixtures/policy/orchestration.toml")}));
        let gate = super::gate::write(&scratch.open(), &["claude"], &scratch.places(), frozen()).unwrap();
        gate["claude"]["refusals"]["claude"].clone()
    };
    assert_eq!(refusal("2026-10-28T10:39:00Z", "2026-10-28T11:59:00Z"), Value::Null);
    let held = refusal("2026-10-28T10:39:00Z", "2026-10-28T10:39:00Z");
    assert!(held.as_str().is_some_and(|why| why.contains("usage snapshot is 81 minutes old even after a refresh")), "{held}");
}

#[test]
fn the_gate_decides_a_recorded_reading_by_setup_token_exactly_as_the_same_reading_by_the_claude_cli() {
    let gate = |source: &str| {
        let mut state: Value = serde_json::from_str(include_str!("fixtures/gate_state.json")).unwrap();
        for row in state["rows"].as_array_mut().unwrap() {
            if row[1] == "claude" && row[3] == "window" {
                row[9] = Value::from(source);
            }
        }
        state["orchestration"] = Value::from(include_str!("../router/fixtures/policy/orchestration.toml"));
        let scratch = Scratch::new(&state);
        super::gate::write(&scratch.open(), &["claude"], &scratch.places(), frozen()).unwrap()
    };
    let cli = gate("claude -p /usage; ~/.claude.json cachedUsageUtilization");
    assert!(cli["claude"]["refusals"].as_object().is_some_and(|refusals| !refusals.is_empty()), "{cli}");
    assert_eq!(gate("Messages response anthropic-ratelimit-unified headers on the account's setup token"), cli);
}

#[test]
fn a_window_a_newer_reading_of_the_account_postdates_the_reset_of_reads_zero_used_as_of_that_reading() {
    let window = |reading_ts: &str| {
        let rows = serde_json::json!([
            ["2026-10-28T09:00:00Z", "claude", "a", "window", "session", 30.0, 300, "2026-10-28T10:00:00Z", null, "headers"],
            [reading_ts, "claude", "a", "window", "weekly_all", 80.0, 10080, "2026-11-03T00:00:00Z", null, "headers"],
        ]);
        let scratch = Scratch::new(&serde_json::json!({"rows": rows, "orchestration": include_str!("../router/fixtures/policy/orchestration.toml")}));
        let gate = super::gate::write(&scratch.open(), &["claude"], &scratch.places(), frozen()).unwrap();
        gate["claude"]["projections"]["claude:a"].as_array().unwrap().iter().find(|w| w["window"] == "5h").unwrap().clone()
    };
    let rolled = window("2026-10-28T11:59:00Z");
    assert_eq!((rolled["used_pct"].as_f64(), rolled["as_of"].as_str(), rolled["age_s"].as_f64()), (Some(0.0), Some("2026-10-28T11:59:00Z"), Some(60.0)), "{rolled}");
    let unseen = window("2026-10-28T09:30:00Z");
    assert_eq!((unseen["used_pct"].as_f64(), unseen["as_of"].as_str()), (Some(30.0), Some("2026-10-28T09:00:00Z")), "{unseen}");
}

#[test]
fn a_claude_account_with_no_profile_on_any_host_is_left_out_of_usage_and_one_on_another_host_stays() {
    let row = |provider: &str, account: &str| serde_json::json!(["2026-10-15T12:00:00Z", provider, account, "window", "session", 20.0, 300, "2026-10-15T15:00:00Z", null, "fixture"]);
    let scratch = Scratch::new(&serde_json::json!({
        "rows": [row("claude", "a"), row("claude", "b"), row("claude", "c"), row("gemini", "x")],
        "orchestration": "[accounts.claude.profiles]\na = { here = {} }\nb = { elsewhere = {} }\n"
    }));
    let shown: Vec<Value> = serde_json::from_str(&scratch.json()).unwrap();
    let pairs: Vec<(&str, &str)> = shown.iter().map(|row| (row["provider"].as_str().unwrap(), row["account"].as_str().unwrap())).collect();
    assert_eq!(pairs, [("claude", "a"), ("claude", "b"), ("gemini", "x")]);
}

#[test]
fn a_session_on_a_token_only_profile_is_the_account_this_host_declares_for_that_profile() {
    let home = tempfile::tempdir().unwrap();
    let (token_home, other) = (home.path().join("profile-d"), home.path().join("elsewhere"));
    fs::create_dir_all(&token_home).unwrap();
    fs::create_dir_all(&other).unwrap();
    let text = format!("[accounts.claude.profiles]\nd = {{ \"here\" = {{ path = \"{}\", token_file = \"{}/token-d\" }} }}\n", token_home.display(), home.path().display());
    let cfg = super::profiles::policy_value(&{
        fs::write(home.path().join("orchestration.toml"), text).unwrap();
        home.path().join("orchestration.toml")
    });
    assert_eq!(super::gate::account_in(&cfg, home.path(), Some(&token_home), "here").as_deref(), Some("d"));
    assert_eq!(super::gate::account_in(&cfg, home.path(), Some(&other), "here"), None);
    assert_eq!(super::gate::account_in(&cfg, home.path(), Some(&token_home), "elsewhere"), None);
}

type UsageRow<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str, f64, i64, &'a str);

fn usage_db(rows: &[UsageRow<'_>]) -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute(CREATE, []).unwrap();
    for (ts, provider, account, kind, name, used, window, resets) in rows {
        db.execute("INSERT INTO usage VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL, 'fixture')", rusqlite::params![ts, provider, account, kind, name, used, window, resets]).unwrap();
    }
    db
}

fn defaults() -> super::ctx::Defaults {
    super::ctx::Defaults {
        min_runs: super::settings::default_value("min_runs").as_u64().unwrap() as usize,
        target_left_claude: super::settings::default_value("pace_target_left_claude"),
        pace_band: super::settings::default_value("pace_band"),
    }
}

fn plan(ctx: &Value) -> Value {
    crate::router::ops::answer(&serde_json::json!({"op": "plan", "ctx": ctx})).unwrap().unwrap()
}

#[test]
fn burn_counts_only_readings_of_the_same_reset_window_within_the_last_three_hours() {
    let reset = "2026-10-17T04:00:00Z";
    let rows = [("2026-10-15T06:00:00Z", 40.0), ("2026-10-15T09:00:00Z", 60.0), ("2026-10-15T12:00:00Z", 63.0)];
    let mut readings: Vec<_> = rows.iter().map(|(ts, used)| (*ts, "codex", "a", "window", "week", *used, 10080, reset)).collect();
    let db = usage_db(&readings);
    let latest = store::latest(&db).unwrap();
    let now = Timestamp::parse_rfc3339("2026-10-15T12:00:00Z").unwrap();
    assert_eq!(super::burn::beta(&db, &latest[0], now).unwrap(), Some(1.0));
    readings.push(("2026-10-15T12:00:00Z", "codex", "a", "window", "week", 63.0, 10080, "2026-10-18T04:00:00Z"));
    let db = usage_db(&readings);
    assert_eq!(super::burn::beta(&db, &store::latest(&db).unwrap()[0], now).unwrap(), None);
}

#[test]
fn cash_per_account_includes_the_binding_gemini_weekly_quota() {
    let now = Timestamp::parse_rfc3339("2026-10-11T12:00:00Z").unwrap();
    let reset = "2026-10-17T04:00:00Z";
    let at = "2026-10-11T12:00:00Z";
    let db = usage_db(&[
        (at, "claude", "a", "window", "weekly_all", 10.0, 10080, reset),
        (at, "claude", "b", "window", "weekly_all", 40.0, 10080, reset),
        (at, "codex", "x", "window", "week", 20.0, 10080, reset),
        (at, "gemini", "g", "window", "group1/weekly", 10.0, 10080, reset),
        (at, "gemini", "g", "window", "group2/weekly", 20.0, 10080, reset),
    ]);
    let latest = store::latest(&db).unwrap();
    let rows: Vec<&store::Row> = latest.iter().collect();
    let mut ctx = super::ctx::live_ctx(&serde_json::json!({}), &[], &serde_json::json!({}), &rows, &db, now, &defaults()).unwrap();
    for (name, price) in [("claude:a", 50.0), ("claude:b", 50.0), ("codex", 50.0), ("gemini", 1.25)] {
        ctx["accounts"][name]["usd_per_week"] = serde_json::json!(price);
    }
    let planned = plan(&ctx);
    let cash: Vec<(String, f64)> = ctx["accounts"].as_object().unwrap().keys().map(|name| (name.clone(), planned["accounts"][name]["available_usd"].as_f64().unwrap())).collect();
    assert_eq!(cash, [("claude:a".to_owned(), 45.0), ("claude:b".to_owned(), 30.0), ("codex".to_owned(), 40.0), ("gemini".to_owned(), 1.0)]);
}

#[test]
fn the_ctx_built_from_a_fixture_ledger_reaches_the_measured_scenarios_plan() {
    let scenario: Value = serde_json::from_str(include_str!("../../../../harness/lib/routing-scenarios/measured.json")).unwrap();
    let now = Timestamp::parse_loose(scenario["now"].as_str().unwrap()).unwrap();
    let tiers: Vec<super::ctx::Tier> = scenario["tiers"].as_array().unwrap().iter().map(|tier| super::ctx::Tier {
        key: tier["key"].as_str().unwrap().to_owned(), model: tier["model"].as_str().unwrap().to_owned(), effort: tier["effort"].as_str().unwrap().to_owned(), start: tier["start"].clone(),
    }).collect();
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, role TEXT, tier TEXT, category TEXT, outcome TEXT, follows TEXT, expects_landing INT, landed INT, item TEXT); CREATE TABLE model_events(ts TEXT, model TEXT, kind TEXT, at TEXT, p REAL, source TEXT, detail TEXT);").unwrap();
    let run_tier = |key: &str| match key {
        "claude:haiku" => "haiku", "claude:opus-medium" => "medium", "claude:opus-high" => "high", "claude:opus-xhigh" => "xhigh",
        "codex:sol-medium" => "gpt-6.1-sol medium", "codex:sol-high" => "gpt-6.1-sol high", other => panic!("{other}"),
    };
    let cells = [("claude:haiku", "tooling", 10, 4), ("codex:sol-medium", "tooling", 20, 19), ("codex:sol-high", "docs", 10, 10), ("codex:sol-high", "tooling", 20, 19),
        ("claude:opus-medium", "docs", 10, 10), ("claude:opus-high", "docs", 10, 10), ("claude:opus-xhigh", "docs", 10, 10)];
    let mut n = 0;
    for (key, category, runs, ok) in cells {
        for i in 0..runs {
            n += 1;
            db.execute("INSERT INTO runs(agent, role, tier, category, outcome, follows, expects_landing, landed) VALUES (?1, 'worker', ?2, ?3, ?4, NULL, 0, 0)", rusqlite::params![format!("a{n}"), run_tier(key), category, if i < ok { "done" } else { "failed" }]).unwrap();
        }
    }
    db.execute_batch(
        "INSERT INTO runs(agent, role, tier, category, outcome, follows, expects_landing, landed) VALUES ('lead', 'lead', 'medium', 'tooling', 'failed', NULL, 0, 0);
         INSERT INTO runs(agent, role, tier, category, outcome, follows, expects_landing, landed) VALUES ('b1', 'worker', 'haiku', 'tooling', 'done', NULL, 1, 0);
         INSERT INTO runs(agent, role, tier, category, outcome, follows, expects_landing, landed) VALUES ('b2', 'worker', 'medium', 'tooling', 'failed', 'b1', 0, 0);
         DELETE FROM runs WHERE agent = 'a10';
         INSERT INTO runs VALUES ('u0', 'worker', 'haiku', 'tooling', 'unclear', NULL, 0, 0, NULL), ('u1', 'worker', 'haiku', 'tooling', 'unclear', NULL, 0, 0, NULL), ('u2', 'worker', 'haiku', 'tooling', 'unclear', NULL, 0, 0, NULL);",
    )
    .unwrap();
    let at = |days: i64| super::py::second(Timestamp::from_unix_nanos(now.unix_nanos() + i128::from(days) * 86_400_000_000_000));
    for (ts, model, kind, when, p) in [
        ("2026-10-01T00:00:00Z", "anthropic:claude-opus-6", "market", at(9), Some(0.4)), ("2026-10-02T00:00:00Z", "anthropic:claude-opus-6", "market", at(5), Some(0.7)),
        ("2026-10-02T00:00:00Z", "anthropic:claude-opus-4", "market", at(1), Some(0.9)), ("2026-10-02T00:00:00Z", "openai:gpt-6.2-sol", "released", at(-1), None),
        ("2026-10-02T00:00:00Z", "google:gemini-4-argon", "market", at(2), Some(0.3)),
    ] {
        db.execute("INSERT INTO model_events VALUES (?1, ?2, ?3, ?4, ?5, 'fixture', '{}')", rusqlite::params![ts, model, kind, when, p]).unwrap();
    }
    let mut live = super::ctx::live_ctx(&serde_json::json!({}), &tiers, &serde_json::json!({}), &[], &db, now, &defaults()).unwrap();
    assert_eq!(live["success"], scenario["success"]);
    let shape = |ctx: &Value| ctx["tiers"].as_array().unwrap().iter().map(|tier| (tier["key"].clone(), tier["runs"].clone(), tier.get("successor").map(|successor| (successor["model"].clone(), successor["p"].clone(), successor["eta_days"].as_f64().map(f64::round))))).collect::<Vec<_>>();
    assert_eq!(shape(&live), shape(&scenario));
    for key in ["billing", "lease", "accounts", "projects", "boxes"] {
        live[key] = scenario[key].clone();
    }
    let result = plan(&live);
    assert_eq!(crate::router::ops::answer(&serde_json::json!({"op": "divergence", "result": result, "expect": scenario["expect"]})).unwrap().unwrap(), Value::Null);
    assert_eq!(result, plan(&scenario));
    let mut blind = live.clone();
    blind["success"] = serde_json::json!({});
    assert_eq!(plan(&blind)["boxes"]["t1"]["tier"], "claude:haiku");
}

#[test]
fn the_rows_entry_takes_one_provider_and_an_optional_no_refresh_and_nothing_else() {
    let words = |text: &str| text.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    assert_eq!(super::rows_request(&words("--rows claude")), Some(("claude", true)));
    assert_eq!(super::rows_request(&words("--rows claude --no-refresh")), Some(("claude", false)));
    assert_eq!(super::rows_request(&words("--rows claude --json")), None);
    assert_eq!(super::rows_request(&words("--rows")), None);
    assert!(super::handles(&words("--rows gemini")));
}
