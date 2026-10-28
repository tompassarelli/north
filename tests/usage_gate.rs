use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

use rusqlite::Connection;
use serde_json::{Value, json};

const STATE: &str = include_str!("../src/usage/fixtures/gate_state.json");
const POLICY: &str = include_str!("../src/router/fixtures/policy/orchestration.toml");
const FROZEN: &str = "2026-10-28T12:00:00Z";
const DRIVER: &str = "import datetime, runpy, sys\nwhen = sys.argv[1]\nclass Frozen(datetime.datetime):\n    @classmethod\n    def now(cls, tz=None):\n        moment = cls.fromisoformat(when)\n        return moment.astimezone(tz) if tz else moment.replace(tzinfo=None)\ndatetime.datetime = Frozen\nsys.argv = ['usage.py'] + sys.argv[2:]\nrunpy.run_path('harness/lib/usage.py', run_name='__main__')\n";

struct Rig {
    _scratch: tempfile::TempDir,
    root: PathBuf,
}

fn cell(value: &Value) -> rusqlite::types::Value {
    match value {
        Value::Null => rusqlite::types::Value::Null,
        Value::String(text) => rusqlite::types::Value::Text(text.clone()),
        number if number.is_i64() => rusqlite::types::Value::Integer(number.as_i64().unwrap()),
        number => rusqlite::types::Value::Real(number.as_f64().unwrap()),
    }
}

fn insert(db: &Connection, sql: &str, rows: &Value) {
    for row in rows.as_array().unwrap() {
        db.execute(sql, rusqlite::params_from_iter(row.as_array().unwrap().iter().map(cell))).unwrap();
    }
}

impl Rig {
    fn new(previous: Option<Value>) -> Self {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().to_path_buf();
        let state: Value = serde_json::from_str(STATE).unwrap();
        let db = Connection::open(root.join("threads.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT) STRICT; \
             CREATE TABLE runs(agent TEXT, tier TEXT, category TEXT, outcome TEXT, expects_landing INTEGER, landed INTEGER, item TEXT, role TEXT, follows TEXT); \
             CREATE TABLE model_events(model TEXT, at TEXT, p REAL, kind TEXT, ts TEXT);",
        )
        .unwrap();
        insert(&db, "INSERT INTO usage VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)", &state["rows"]);
        insert(&db, "INSERT INTO runs VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)", &state["runs"]);
        insert(&db, "INSERT INTO model_events VALUES (?1, ?2, ?3, ?4, ?5)", &state["events"]);
        fs::write(root.join("orchestration.toml"), POLICY).unwrap();
        for side in ["python", "rust"] {
            fs::create_dir_all(root.join(side).join("agents")).unwrap();
            if let Some(previous) = &previous {
                fs::write(root.join(side).join("agents/usage-gate.json"), previous.to_string()).unwrap();
            }
        }
        Self { _scratch: scratch, root }
    }

    fn environment(&self, command: &mut Command, side: &str) {
        command
            .env("HOME", &self.root)
            .env("THREADS_DB", self.root.join("threads.db"))
            .env("AGENTS_ORCHESTRATION", self.root.join("orchestration.toml"))
            .env("AGENTS_OPERATOR_DIR", self.root.join("op"))
            .env("XDG_STATE_HOME", self.root.join(side))
            .env("NORTH_PACE_BIN", env!("CARGO_BIN_EXE_north"))
            .env_remove("CLAUDE_CONFIG_DIR");
    }

    fn python(&self) -> Output {
        let mut command = Command::new("python3");
        command.current_dir(env!("CARGO_MANIFEST_DIR")).args(["-I", "-c", DRIVER, "2026-10-28T12:00:00+00:00", "--write-pace-gate"]);
        self.environment(&mut command, "python");
        command.output().unwrap()
    }

    fn rust(&self) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command.args(["agents", "usage", "--write-pace-gate"]).env("USAGE_NOW", FROZEN);
        self.environment(&mut command, "rust");
        command.output().unwrap()
    }

    fn file(&self, side: &str) -> Value {
        serde_json::from_str(&fs::read_to_string(self.root.join(side).join("agents/usage-gate.json")).unwrap()).unwrap()
    }
}

fn printed(output: &Output) -> Value {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

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
fn the_pace_gate_rust_writes_equals_the_one_usage_py_writes_on_a_recorded_ledger() {
    let rig = Rig::new(None);
    let (python, rust) = (printed(&rig.python()), printed(&rig.rust()));
    assert!(python["claude"]["projections"].as_object().is_some_and(|projections| projections.len() == 2), "{python}");
    assert!(python["claude"]["refusals"]["claude:b"].is_string(), "{python}");
    same(&python, &rust, "stdout");
    same(&rig.file("python"), &rig.file("rust"), "file");
    same(&rust, &rig.file("rust"), "written");
}

#[test]
fn a_rewritten_gate_keeps_each_providers_controls_and_drops_providers_outside_the_catalog() {
    let rig = Rig::new(Some(json!({"claude": {"controls": {"hold": true}, "refusal": "old"}, "codex": {"refusal": "retired"}})));
    let (python, rust) = (printed(&rig.python()), printed(&rig.rust()));
    assert_eq!(rust["claude"]["controls"], json!({"hold": true}));
    assert!(rust.get("codex").is_none() && python.get("codex").is_none(), "{rust}");
    same(&python, &rust, "stdout");
}
