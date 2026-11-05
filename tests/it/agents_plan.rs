use north_core::foundation::shim;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::Connection;
use serde_json::Value;
use tempfile::TempDir;
const FIXTURE_TIERS: &str = r#"mode = "split"
[tiers."claude:haiku"]
model = "claude-haiku-5-5"
effort = "high"
agent = "worker-haiku"
role = "mechanical"
start = 0
[tiers."gemini:3.8-flash-high"]
model = "gemini-3.8-flash"
effort = "high"
role = "middle, Gemini side"
start = 20
[tiers."claude:opus-medium"]
model = "claude-opus-5-5"
effort = "medium"
agent = "worker"
role = "top"
start = 75
[tiers."claude:opus-high"]
model = "claude-opus-5-5"
effort = "high"
agent = "worker-high"
role = "planning and architecture"
start = 95
"#;

struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let fixture = Self { dir: tempfile::tempdir().unwrap() };
        fs::create_dir_all(fixture.path("bin")).unwrap();
        fixture.script("bin/claude", "#!/usr/bin/env bash\n[[ \"$*\" == \"auth status\" ]] || exit 97\nprintf '{\"loggedIn\": %s}\\n' \"${FAKE_CLAUDE_IN:-true}\"\n");
        fixture.script("bin/update-status", "#!/usr/bin/env bash\nprintf 'STALE: machine-update fixture snapshot is stale\\n'\n");
        fs::write(fixture.path("costs.toml"), "[[service]]\nid = \"claude-a\"\nkind = \"ai-plan\"\nusd_per_week = 50\n").unwrap();
        Connection::open(fixture.path("threads.db")).unwrap();
        fixture.orchestration(FIXTURE_TIERS);
        fixture
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn script(&self, name: &str, body: &str) {
        let path = self.path(name);
        shim::write_executable(&path, body);
    }

    fn orchestration(&self, text: &str) {
        fs::write(self.path("orchestration.toml"), text).unwrap();
    }

    fn plan(&self, arguments: &[&str], signed_in: bool) -> Output {
        let path = format!("{}:{}", self.path("bin").display(), std::env::var("PATH").unwrap_or_default());
        Command::new(env!("CARGO_BIN_EXE_north"))
            .args(["agents", "plan"])
            .args(arguments)
            .env("NORTH_HOME", env!("CARGO_MANIFEST_DIR"))
            .env("PATH", path)
            .env("AGENTS_CLAUDE_BIN", self.path("bin/claude"))
            .env("AGENTS_ORCHESTRATION", self.path("orchestration.toml"))
            .env("THREADS_DB", self.path("threads.db"))
            .env("XDG_STATE_HOME", self.path("state"))
            .env("FAKE_CLAUDE_IN", signed_in.to_string())
            .output()
            .unwrap()
    }

    fn json(&self, arguments: &[&str]) -> (i32, Value) {
        let output = self.plan(&[&["--json"], arguments].concat(), true);
        (output.status.code().unwrap(), serde_json::from_slice(&output.stdout).unwrap_or(Value::Null))
    }
}

fn scenario(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/north-pace/src/router/fixtures/scenarios").join(format!("{name}.json"))
}

#[test]
fn every_recorded_scenario_replays_to_its_expected_decisions() {
    let fixture = Fixture::new();
    for name in ["burn", "conserve", "new-model", "deadline", "two-claude", "reserve", "thursday-behind", "capacity-astra", "capacity-floor"] {
        let path = scenario(name);
        let (code, actual) = fixture.json(&["--at", path.to_str().unwrap()]);
        assert_eq!(code, 0, "replay {name}");
        let expected: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        for (kind, label, place) in [("states", "state", "accounts"), ("tiers", "tier", "boxes"), ("fast", "fast", "boxes")] {
            for (key, want) in expected["expect"][kind].as_object().into_iter().flatten() {
                assert_eq!(&actual[place][key][label], want, "{name}: {kind} {key}");
            }
        }
    }
}

