use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::Connection;
use serde_json::Value;

use super::store::{self, Source};
use super::{Places, costs, report, table};
use crate::foundation::clock::Timestamp;

const STATES: &str = include_str!("fixtures/states.json");
const CREATE: &str = "CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT";
const FROZEN: &str = "2026-10-28T12:00:00Z";
const DRIVER: &str = "import datetime, runpy, sys\nwhen = sys.argv[1]\nclass Frozen(datetime.datetime):\n    @classmethod\n    def now(cls, tz=None):\n        moment = cls.fromisoformat(when)\n        return moment.astimezone(tz) if tz else moment.replace(tzinfo=None)\ndatetime.datetime = Frozen\nsys.argv = ['usage.py'] + sys.argv[2:]\nrunpy.run_path('harness/lib/usage.py', run_name='__main__')\n";

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

    fn python(&self, arguments: &[&str]) -> String {
        python(self.dir.path(), Some(&self.db()), &self.places(), arguments)
    }
}

fn frozen() -> Timestamp {
    Timestamp::parse_rfc3339(FROZEN).unwrap()
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
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

fn python(home: &Path, threads_db: Option<&Path>, places: &Places, arguments: &[&str]) -> String {
    let mut command = Command::new("python3");
    command
        .current_dir(root())
        .args(["-I", "-c", DRIVER, "2026-10-28T12:00:00+00:00"])
        .args(arguments)
        .env("HOME", home)
        .env("AGENTS_ORCHESTRATION", &places.policy)
        .env("AGENTS_OPERATOR_DIR", &places.operator)
        .env("XDG_STATE_HOME", &places.state);
    if let Some(path) = threads_db {
        command.env("THREADS_DB", path);
    }
    let output = command.output().unwrap();
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn agents_usage_json_is_byte_equal_between_python_and_rust_on_three_recorded_sampler_states() {
    for state in &states()[..3] {
        let scratch = Scratch::new(state);
        assert_eq!(scratch.json(), scratch.python(&["--json"]), "{}", state["name"]);
        assert!(serde_json::from_str::<Vec<Value>>(&scratch.json()).unwrap().len() > 1, "{}", state["name"]);
    }
}

#[test]
fn the_agents_usage_table_is_byte_equal_between_python_and_rust_on_four_recorded_sampler_states() {
    for state in &states() {
        let scratch = Scratch::new(state);
        assert_eq!(scratch.text(), scratch.python(&[]), "{}", state["name"]);
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
fn a_ledger_without_a_usage_table_or_file_prints_what_python_prints() {
    let scratch = Scratch::new(&serde_json::json!({"rows": [], "orchestration": ""}));
    let bare = scratch.dir.path().join("bare.db");
    Connection::open(&bare).unwrap().execute("CREATE TABLE other(x)", []).unwrap();
    for db in [bare, scratch.dir.path().join("absent.db")] {
        let source = Source::locate(scratch.dir.path(), Some(db.clone().into()));
        let open = || store::open(&source, &|| {}).unwrap();
        let (json, (text, code)) = (report(&open(), &scratch.places().policy, frozen()).unwrap(), table::render(&open(), &scratch.places(), frozen()).unwrap());
        assert_eq!((json + "\n", text + "\n", code), (python(scratch.dir.path(), Some(&db), &scratch.places(), &["--json"]), python(scratch.dir.path(), Some(&db), &scratch.places(), &[]), 1));
    }
}

#[test]
fn a_remote_host_reads_the_mirrored_snapshot_and_prints_what_python_prints() {
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
    assert_eq!(actual, python(home.path(), None, &places, &["--json"]));
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
