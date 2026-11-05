use std::fs;

use rusqlite::Connection;
use serde_json::json;

use super::{View, render};
use crate::foundation::clock::Timestamp;
use crate::usage::Places;

const CREATE: &str = "CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT";
const NOW: &str = "2026-11-05T19:20:00Z";
const BANGKOK: i64 = 7 * 3600;

type Reading<'a> = (&'a str, &'a str, &'a str, &'a str, &'a str, Option<f64>, Option<i64>, Option<&'a str>, Option<f64>);

const READINGS: [Reading; 22] = [
    ("2026-11-05T19:19:00Z", "claude", "a", "window", "session", Some(30.0), Some(300), Some("2026-10-31T06:40:00Z"), None),
    ("2026-11-05T17:19:00Z", "claude", "a", "window", "weekly_all", Some(64.6), Some(10080), Some("2026-11-11T02:00:00Z"), None),
    ("2026-11-05T19:19:00Z", "claude", "a", "window", "weekly_all", Some(65.0), Some(10080), Some("2026-11-11T02:00:00Z"), None),
    ("2026-11-05T19:19:00Z", "claude", "a", "window", "weekly_scoped/Fable", Some(0.0), Some(10080), Some("2026-11-11T02:00:00Z"), None),
    ("2026-11-05T19:19:00Z", "claude", "a", "billing", "extra_usage", None, None, None, Some(0.0)),
    ("2026-11-05T19:19:00Z", "claude", "b", "window", "session", Some(0.0), Some(300), Some("2026-10-31T17:40:00Z"), None),
    ("2026-11-05T19:19:00Z", "claude", "b", "window", "weekly_all", Some(100.0), Some(10080), Some("2026-11-07T11:00:00Z"), None),
    ("2026-11-05T19:19:00Z", "claude", "b", "billing", "plan", None, None, None, Some(1.0)),
    ("2026-11-05T16:20:00Z", "claude", "c", "window", "session", Some(18.0), Some(300), Some("2026-11-05T22:02:00Z"), None),
    ("2026-11-05T16:20:00Z", "claude", "c", "window", "weekly_all", Some(4.0), Some(10080), Some("2026-11-12T11:30:00Z"), None),
    ("2026-11-05T19:19:00Z", "claude", "d", "window", "session", Some(10.0), Some(300), Some("2026-11-05T20:12:00Z"), None),
    ("2026-11-05T17:19:00Z", "claude", "d", "window", "weekly_all", Some(95.0), Some(10080), Some("2026-11-10T07:00:00Z"), None),
    ("2026-11-05T19:19:00Z", "claude", "d", "window", "weekly_all", Some(97.0), Some(10080), Some("2026-11-10T07:00:00Z"), None),
    ("2026-11-05T19:19:00Z", "claude", "d", "window", "weekly_scoped/Fable", Some(12.0), Some(10080), Some("2026-11-10T07:00:00Z"), None),
    ("2026-11-05T19:19:00Z", "claude", "d", "billing", "extra_usage", None, None, None, Some(3.5)),
    ("2026-11-05T19:11:00Z", "gemini", "default", "window", "gemini-pro-agent/recorded-quota", Some(100.0), None, Some("2026-10-17T08:38:12Z"), None),
    ("2026-11-05T19:11:00Z", "gemini", "default", "window", "Claude and GPT models/5h", Some(0.0), Some(300), Some("2026-11-06T00:23:18Z"), None),
    ("2026-11-05T19:11:00Z", "gemini", "default", "window", "Gemini Models/5h", Some(0.0), Some(300), Some("2026-11-06T00:23:18Z"), None),
    ("2026-11-05T19:11:00Z", "gemini", "default", "window", "Claude and GPT models/weekly", Some(50.86), Some(10080), Some("2026-11-10T01:55:36Z"), None),
    ("2026-11-05T19:11:00Z", "gemini", "default", "window", "Gemini 4 Argon/weekly", Some(100.0), Some(10080), Some("2026-11-09T18:40:14Z"), None),
    ("2026-11-05T19:11:00Z", "gemini", "default", "window", "Gemini Models/weekly", Some(5.16), Some(10080), Some("2026-11-09T18:43:04Z"), None),
    ("2026-11-05T19:11:00Z", "gemini", "default", "billing", "extra_usage", None, None, None, Some(0.0)),
];

const COSTS: &str = r#"[[service]]
id = "claude-a"
kind = "ai-plan"
billing = "fixed"
usd_per_week = 50.0

[[service]]
id = "promo-credit"
kind = "ai-plan"
billing = "prepaid"
usd_per_week = 0.0
initial = 250.0
expires = 2026-11-05T07:59:00Z

[[service]]
id = "hetzner-gex45"
name = "gex45"
kind = "cloud"
billing = "fixed"
usd_per_week = 57.02

[[service]]
id = "digitalocean"
name = "DigitalOcean"
kind = "cloud"
billing = "metered"
usd_per_week = 0.0

[[service]]
id = "hetzner-ccx33"
kind = "cloud"
billing = "metered"
usd_per_week = 0.0
status = "deleted 2026-11-05 10:44 PM local"

[[service]]
id = "vast"
kind = "gpu-rental"
billing = "prepaid"
usd_per_week = 0.0
status = "deprecated"
"#;

const GOLDEN: &str = "\
Claude       5 hours        week             week resets   outlook
a  Max 20x   —              65%              Wed 9:00 AM   lasts to reset
b  Max 20x   —              100% (full)      Sat 6:00 PM   out until reset
c  Max 5x    18% (2h 42m)   4%               Thu 6:30 PM   no burn rate yet; read 3 hours ago
d  Max 20x   10% (52m)      97%, Fable 12%   Tue 2:00 PM   runs out in 3h 00m; 2 full and 1 five-hour resets left; extra usage 3.5

Gemini       week 51% (Claude and GPT models), 100% (Gemini 4 Argon), 5% (Gemini Models)   read 9 min ago
This week    $118.82: $107.02 fixed, $11.80 metered (gex45, DigitalOcean $8.09)";

struct Fixture {
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self { dir: tempfile::tempdir().unwrap() };
        let places = fixture.places();
        fs::write(&places.policy, "[accounts.claude]\nplans = { a = \"max-20x\", c = \"max-5x\", d = \"max-20x\" }\n").unwrap();
        fs::create_dir_all(&places.operator).unwrap();
        fs::write(places.operator.join("costs.toml"), COSTS).unwrap();
        fs::create_dir_all(places.state.join("agents/costs")).unwrap();
        let digitalocean = json!({"period_start": "2026-11-01T00:00:00Z", "period_end": "2026-12-01T00:00:00Z", "usd_spent_period": 8.09, "usd_balance": 8.09, "read_at": "2026-11-05T19:11:00Z", "source": "api"});
        fs::write(places.state.join("agents/costs/digitalocean.json"), digitalocean.to_string()).unwrap();
        let account = |label: &str, full: i64, five_hour: i64| {
            json!({"account_label": label, "read_at": "2026-11-05T19:00:00Z", "resets": {"full": {"count": full, "expires": "2026-11-20T00:00:00Z"}, "five_hour": {"count": five_hour}}})
        };
        fs::create_dir_all(places.state.join("agents/usage")).unwrap();
        fs::write(places.state.join("agents/usage/claude-web.json"), json!({"accounts": {"x": account("b", 0, 0), "y": account("d", 2, 1)}}).to_string()).unwrap();
        fixture
    }

    fn places(&self) -> Places {
        let root = self.dir.path();
        Places { policy: root.join("orchestration.toml"), operator: root.join("op"), state: root.join("state") }
    }
}

fn ledger(readings: &[Reading]) -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute(CREATE, []).unwrap();
    for (ts, provider, account, kind, name, used, window, resets_at, amount) in readings {
        let source = if *name == "plan" { "max-20x" } else { "fixture" };
        db.execute("INSERT INTO usage VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)", rusqlite::params![ts, provider, account, kind, name, used, window, resets_at, amount, source]).unwrap();
    }
    db
}

fn now() -> Timestamp {
    Timestamp::parse_rfc3339(NOW).unwrap()
}

#[test]
fn the_default_view_is_one_aligned_row_per_claude_account_in_local_time_without_lapsed_windows_or_retired_services() {
    let fixture = Fixture::new();
    let places = fixture.places();
    let db = ledger(&READINGS);
    let (text, code) = render(&View { db: &db, places: &places, now: now(), offset_seconds: BANGKOK }).unwrap();
    assert_eq!((text.as_str(), code), (GOLDEN, 0), "\n{text}");
}

#[test]
fn with_no_readings_the_default_view_says_so_above_the_cost_line_and_exits_1() {
    let fixture = Fixture::new();
    let places = fixture.places();
    let db = ledger(&[]);
    let (text, code) = render(&View { db: &db, places: &places, now: now(), offset_seconds: BANGKOK }).unwrap();
    let want = "No usage readings yet; run `north usage --refresh`.\nThis week   $118.82: $107.02 fixed, $11.80 metered (gex45, DigitalOcean $8.09)";
    assert_eq!((text.as_str(), code), (want, 1));
}
