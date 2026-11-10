use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;

use super::store::{self, Source};
use super::reset::Outcome;
use super::Places;
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
        Places { policy: root.join("orchestration.toml"), costs: root.join("op/costs.toml"), state: root.join("state") }
    }

    fn open(&self) -> Connection {
        store::open(&Source::locate(self.dir.path(), Some(self.db().into())), &|| {}).unwrap()
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
fn the_ctx_built_for_the_router_equals_the_one_the_python_plan_usage_built_on_a_recorded_ledger_with_runs() {
    let state: Value = serde_json::from_str(include_str!("fixtures/gate_state.json")).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let (db_path, policy) = (scratch.path().join("threads.db"), scratch.path().join("orchestration.toml"));
    ledger(&db_path, &state["rows"]);
    let db = Connection::open(&db_path).unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, tier TEXT, category TEXT, outcome TEXT, expects_landing INTEGER, landed INTEGER, item TEXT, role TEXT, follows TEXT);").unwrap();
    for row in state["runs"].as_array().unwrap() {
        let cells = row.as_array().unwrap().iter().map(|cell| match cell {
            Value::Null => rusqlite::types::Value::Null,
            Value::String(text) => rusqlite::types::Value::Text(text.clone()),
            number if number.is_i64() => rusqlite::types::Value::Integer(number.as_i64().unwrap()),
            number => rusqlite::types::Value::Real(number.as_f64().unwrap()),
        });
        db.execute("INSERT INTO runs VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)", rusqlite::params_from_iter(cells)).unwrap();
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
        gate["claude"]["windows"]["claude:a"].as_array().unwrap().iter().find(|w| w["window"] == "5h").unwrap().clone()
    };
    let rolled = window("2026-10-28T11:59:00Z");
    assert_eq!((rolled["used_pct"].as_f64(), rolled["as_of"].as_str(), rolled["age_s"].as_f64()), (Some(0.0), Some("2026-10-28T11:59:00Z"), Some(60.0)), "{rolled}");
    let unseen = window("2026-10-28T09:30:00Z");
    assert_eq!((unseen["used_pct"].as_f64(), unseen["as_of"].as_str()), (Some(30.0), Some("2026-10-28T09:00:00Z")), "{unseen}");
}

/// Opened by north#1065: a subscription account whose provider cache carries no scoped Fable row still needs
/// a Fable meter, because Fable draws on the account's shared weekly window until the provider opens its own.
#[test]
fn every_subscription_account_has_a_fable_meter_read_from_its_shared_weekly_window() {
    let rows = serde_json::json!([
        ["2026-10-28T11:50:00Z", "claude", "a", "window", "session", 10.0, 300, "2026-10-28T16:00:00Z", null, "x"],
        ["2026-10-28T11:50:00Z", "claude", "a", "window", "weekly_all", 40.0, 10080, "2026-11-03T00:00:00Z", null, "x"],
        ["2026-10-28T11:50:00Z", "claude", "b", "window", "session", 5.0, 300, "2026-10-28T16:00:00Z", null, "x"],
        ["2026-10-28T11:50:00Z", "claude", "b", "window", "weekly_all", 60.0, 10080, "2026-11-03T00:00:00Z", null, "x"],
    ]);
    let orchestration = "[accounts.claude.profiles.a]\nhere = {}\n[accounts.claude.profiles.b]\nhere = {}\n";
    let scratch = Scratch::new(&serde_json::json!({"rows": rows, "orchestration": orchestration}));
    let report = super::report(&scratch.open(), &scratch.places().policy, frozen()).unwrap();
    let rows: Vec<Value> = serde_json::from_str(&report).unwrap();
    for (account, used) in [("a", 40.0), ("b", 60.0)] {
        let fable = rows
            .iter()
            .find(|row| row["account"] == account && row["name"] == "weekly_scoped/Fable")
            .unwrap_or_else(|| panic!("{account} has no Fable meter: {report}"));
        assert_eq!(fable["used_pct"], serde_json::json!(used), "{fable}");
        assert_eq!(fable["resets_at"], serde_json::json!("2026-11-03T00:00:00Z"), "{fable}");
    }
}

/// A Fable meter the provider does report has no reset of its own (`resets_at: null` in the recorded cache), so
/// the table used to drop it. The meter still shows its used percent beside the account's week.
#[test]
fn the_table_shows_a_reported_fable_meter_even_when_it_has_no_reset_of_its_own() {
    let rows = serde_json::json!([
        ["2026-10-28T11:50:00Z", "claude", "a", "window", "session", 12.5, 300, "2026-10-28T16:00:00Z", null, "claude -p /usage"],
        ["2026-10-28T11:50:00Z", "claude", "a", "window", "weekly_all", 40.0, 10080, "2026-11-03T00:00:00Z", null, "claude -p /usage"],
        ["2026-10-28T11:50:00Z", "claude", "a", "window", "weekly_scoped/Opus", 20.0, 10080, "2026-11-03T00:00:00Z", null, "claude -p /usage"],
        ["2026-10-28T11:50:00Z", "claude", "a", "window", "weekly_scoped/Fable", 3.0, 10080, null, null, "claude -p /usage"],
    ]);
    let orchestration = "[accounts.claude.profiles.a]\nhere = {}\n";
    let web = serde_json::json!({
        "accounts": {
            "h": {
                "account": "h",
                "account_label": "a",
                "read_at": "2026-10-28T11:00:00Z",
                "valid_until": "2026-11-01T00:00:00Z",
                "source": "claude.ai",
                "resets": {"full": {"count": 1, "expires": "2026-11-02"}, "five_hour": {"count": 0, "expires": null}},
            }
        }
    });
    let scratch = Scratch::new(&serde_json::json!({"rows": rows, "orchestration": orchestration, "web": web}));
    let latest = super::store::latest(&scratch.open()).unwrap();
    let fable = latest.iter().find(|row| row.text("name") == Some("weekly_scoped/Fable")).expect("the Fable row is read");
    assert_eq!(fable.text("resets_at"), Some("2026-11-03T00:00:00Z"), "the Fable meter takes the account's weekly reset");
    let (text, code) = super::table::render(&super::table::View { db: &scratch.open(), places: &scratch.places(), now: frozen(), offset_seconds: 0 }).unwrap();
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("Opus 20%"), "{text}");
    assert!(text.contains("Fable 3%"), "{text}");
    let offered = super::reset::offered(&scratch.places().state, "a", frozen()).unwrap().expect("the cached grant is read");
    assert_eq!((offered.full, offered.five_hour, offered.expires.as_deref()), (1.0, 0.0, Some("2026-11-02")));
    assert_eq!(super::reset::assess(99.0, Some(&offered)), Outcome::RefusedTerms);
    assert_eq!(super::reset::assess(40.0, Some(&offered)), Outcome::BelowFloor);
    assert_eq!(super::reset::assess(99.0, None), Outcome::NoGrant);
    let log = scratch.dir.path().join("resets.jsonl");
    super::reset::record(&log, "a", frozen(), 99.0, 99.0, Some(&offered), &Outcome::RefusedTerms).unwrap();
    let line: Value = serde_json::from_str(fs::read_to_string(&log).unwrap().trim()).unwrap();
    assert_eq!(line["account"], "a");
    assert_eq!(line["at"], "2026-10-28T12:00:00Z");
    assert_eq!(line["meter_before"], 99.0);
    assert_eq!(line["meter_after"], 99.0);
    assert_eq!(line["resets"]["full"], 1.0);
    assert_eq!(line["outcome"], "refused-terms");
}

#[test]
fn an_api_key_account_profile_gets_the_shared_harness_so_its_workers_run_under_the_same_hooks() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".claude")).unwrap();
    fs::write(home.path().join(".claude/settings.json"), "{}").unwrap();
    let keyed = home.path().join("profiles/claude-api");
    let cfg = serde_json::json!({"accounts": {"claude": {"api_keys": {"api": {"laptop": {"path": keyed.to_str().unwrap()}}}}}});
    super::profiles::sync(&cfg, "laptop", home.path()).unwrap();
    assert_eq!(fs::read_link(keyed.join("settings.json")).unwrap(), home.path().join(".claude/settings.json"));
}

#[test]
fn cpx62_declares_every_profile_so_sync_creates_each_one_with_hooks_where_it_had_none() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir_all(home.path().join(".claude")).unwrap();
    fs::write(home.path().join(".claude/settings.json"), r#"{"hooks": {"PreToolUse": []}}"#).unwrap();
    let path = |name: &str| home.path().join("profiles").join(name).to_str().unwrap().to_owned();
    let cfg = serde_json::json!({"accounts": {"claude": {
        "profiles": {"a": {"cpx62": {"path": path("claude-a")}}, "d": {"cpx62": {"path": path("claude-d")}}},
        "api_keys": {"api": {"cpx62": {"path": path("claude-api")}}},
    }}});

    let before = super::profiles::unhooked(&cfg, "cpx62", home.path());
    assert!(before.iter().any(|why| why.starts_with("api: ") && why.contains("claude-api/settings.json")), "{before:?}");
    super::profiles::sync(&cfg, "cpx62", home.path()).unwrap();
    assert_eq!(super::profiles::unhooked(&cfg, "cpx62", home.path()), Vec::<String>::new());
    for name in ["claude-a", "claude-d", "claude-api"] {
        assert!(home.path().join("profiles").join(name).join("settings.json").exists(), "{name}");
    }
}

#[test]
fn a_login_still_reporting_the_retired_max_20x_tier_shows_as_ultra_10x() {
    let nowhere = Path::new("/nonexistent/orchestration.toml");
    let label = |observed: &str| match super::settings::plan(nowhere, "claude", Some("a"), Some(observed)) {
        Some(super::settings::Plan::Named { name, .. }) => name,
        _ => String::new(),
    };
    assert_eq!(label("max-20x"), "Claude Ultra 10x");
    assert_eq!(label("max-5x"), "Claude Max 5x");
}

#[test]
fn the_account_view_of_the_2026_11_10_afternoon_readings_fits_a_screen_and_shows_no_raw_fields_or_negative_durations() {
    let state: Value = serde_json::from_str(include_str!("fixtures/afternoon.json")).unwrap();
    let scratch = Scratch::new(&state);
    let placement: std::collections::BTreeMap<String, super::Placement> = [
        ("a", super::State::Out, "week window at the provider's limit"),
        ("b", super::State::Held, "held until c is spent (1% of its week left)"),
        ("c", super::State::Open, ""),
        ("d", super::State::Open, ""),
        ("e", super::State::Out, "5h window at the provider's limit"),
    ]
    .into_iter()
    .map(|(account, state, reason)| (account.to_owned(), super::Placement { state, reason: reason.to_owned() }))
    .collect();
    let grants = [
        super::ApiGrant { name: "gmail".into(), left_usd: 126.53, expires: Some("2026-12-10".into()), state: super::State::Open, reason: String::new() },
        super::ApiGrant { name: "proton".into(), left_usd: 0.0, expires: Some("2026-11-05".into()), state: super::State::Out, reason: "the provider refused a turn for credit".into() },
    ];
    let db = scratch.open();
    let places = scratch.places();
    let now = Timestamp::parse_rfc3339("2026-11-10T08:44:00Z").unwrap();
    let text = super::summary(&super::SummaryView { db: &db, places: &places, now, offset_seconds: 7 * 3600, placement: &placement, grants: &grants }).unwrap();

    for line in text.lines() {
        assert!(line.chars().count() <= 100, "{} columns: {line}", line.chars().count());
        assert!(!line.contains('='), "{line}");
        assert!(!line.contains("resets in"), "{line}");
        assert!(!line.split_whitespace().any(|word| word.starts_with('-') && word.chars().skip(1).any(|c| c.is_ascii_digit()) && word.contains('h')), "{line}");
    }
    let row = |name: &str| text.lines().find(|line| line.split_whitespace().next() == Some(name)).unwrap_or_else(|| panic!("no row for {name} in\n{text}")).split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(row("a"), "a Ultra 10x out — — 0% Thu 9:00 AM week full");
    assert_eq!(row("d"), "d Ultra 10x open 90% 8:00 PM 98% Tue 3:00 PM");
    assert_eq!(row("e"), "e Max 5x out 0% 5:30 PM 42% Wed 10:00 AM 5h full");
    assert!(row("b").contains("held") && row("b").ends_with("held until c is spent"), "{}", row("b"));
    assert_eq!(row("gmail"), "gmail open $126.53 Dec 10");
    assert!(row("proton").starts_with("proton out $0.00 Nov 5 "), "{}", row("proton"));
}
