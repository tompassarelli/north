use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::Connection;
use serde_json::{Value, json};
use tempfile::TempDir;
use time::OffsetDateTime;

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

const LIVE_ORCHESTRATION: &str = r#"mode = "split"
[projects.galileo]
priority = 1
share = 25
[accounts.claude]
target_left = 15
band = [10, 20]
[tiers."claude:haiku"]
model = "claude-haiku-5-5"
effort = "high"
start = 0
[tiers."claude:opus-medium"]
model = "claude-opus-5-5"
effort = "medium"
start = 20
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
        fs::write(&path, body).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
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

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn scenario(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("harness/lib/routing-scenarios").join(format!("{name}.json"))
}

fn bands(plan: &Value) -> Value {
    json!(plan["bands"].as_array().unwrap().iter().map(|band| json!([band[0].as_f64().unwrap(), band[1].as_f64().unwrap(), band[2]])).collect::<Vec<_>>())
}

fn same_bands(plan: &Value) -> bool {
    bands(plan) == json!([[0.0, 20.0, "claude:haiku"], [20.0, 95.0, "claude:opus-medium"], [95.0, 100.0, "claude:opus-high"]])
}

fn iso(at: OffsetDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        at.year(),
        u8::from(at.month()),
        at.day(),
        at.hour(),
        at.minute(),
        at.second()
    )
}

#[test]
fn the_plan_reports_the_providers_signed_in_now_and_the_diagnostic_of_update_status() {
    let fixture = Fixture::new();
    let output = fixture.plan(&["--json"], true);
    assert_eq!(stderr(&output).trim_end(), "STALE: machine-update fixture snapshot is stale");
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(plan["mode"], "split");
    assert_eq!(plan["usable"], json!(["claude"]));
    assert!(same_bands(&plan), "{plan}");
    assert_eq!(plan["escalation"], json!(["claude:haiku", "claude:opus-medium", "claude:opus-high"]));
}

#[test]
fn a_provider_without_a_sign_in_check_is_dropped() {
    let fixture = Fixture::new();
    let readout = stdout(&fixture.plan(&[], true));
    assert!(readout.contains("gemini: dropped, not signed in (no sign-in check for this provider)"), "{readout}");
}

#[test]
fn claude_only_keeps_the_same_bands_and_a_provider_only_mode_with_no_check_fails() {
    let fixture = Fixture::new();
    let split = fixture.json(&[]).1;
    let (code, only) = fixture.json(&["--mode", "claude-only"]);
    assert_eq!(code, 0);
    assert_eq!(only["usable"], json!(["claude"]));
    assert_eq!(bands(&only), bands(&split));
    assert_eq!(only["escalation"], split["escalation"]);
    let (code, none) = fixture.json(&["--mode", "gemini-only"]);
    assert_ne!(code, 0);
    assert_eq!((none["usable"].clone(), none["bands"].clone(), none["escalation"].clone()), (json!([]), json!([]), json!([])));
    let unknown = fixture.plan(&["--mode", "nonsense"], true);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(stderr(&unknown).contains("unknown mode 'nonsense'; use split, claude-only, gemini-only"));
}

#[test]
fn a_signed_out_claude_leaves_no_usable_provider() {
    let fixture = Fixture::new();
    let output = fixture.plan(&["--json"], false);
    assert_ne!(output.status.code(), Some(0));
    let plan: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!((plan["usable"].clone(), plan["bands"].clone(), plan["escalation"].clone()), (json!([]), json!([]), json!([])));
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

fn replay_text(fixture: &Fixture, name: &str) -> String {
    let path = scenario(name);
    stdout(&fixture.plan(&["--at", path.to_str().unwrap()], true))
}

#[test]
fn a_replay_prints_the_thursday_projection_and_flags_only_the_account_that_is_behind() {
    let fixture = Fixture::new();
    let behind = replay_text(&fixture, "thursday-behind");
    assert!(behind.contains("FLAG codex drain behind") && behind.contains("projected 45.0% left at Fri 23:59 ET (band 10-20)"), "{behind}");
    let reserve = replay_text(&fixture, "reserve");
    assert!(!reserve.contains("FLAG codex drain behind") && reserve.contains("projected 18.0% left at Fri 23:59 ET (band 10-20)"), "{reserve}");
}

#[test]
fn a_replay_prints_provider_capacity_as_unavailable_and_the_astra_fallback() {
    let fixture = Fixture::new();
    let floor = replay_text(&fixture, "capacity-floor");
    assert!(floor.contains("account codex: unavailable (provider capacity, since "), "{floor}");
    assert!(floor.contains("box t1 (north tooling d30): claude:haiku,"), "{floor}");
    assert!(floor.contains("account codex: state at or over pace, used 89"), "{floor}");
    let astra = replay_text(&fixture, "capacity-astra");
    assert!(astra.contains("box t1 (north tooling d30): codex:sol-high on gpt-6-astra medium (capacity fallback)"), "{astra}");
}

#[test]
fn a_replay_runs_update_status_like_the_live_plan() {
    let fixture = Fixture::new();
    let path = scenario("burn");
    let output = fixture.plan(&["--at", path.to_str().unwrap()], true);
    assert_eq!(stderr(&output).trim_end(), "STALE: machine-update fixture snapshot is stale");
}

#[test]
fn a_replay_that_misses_its_expectation_exits_1_with_the_mismatch() {
    let fixture = Fixture::new();
    let mut ctx: Value = serde_json::from_slice(&fs::read(scenario("burn")).unwrap()).unwrap();
    ctx["expect"] = json!({"states": {"codex": "no such state"}});
    fs::write(fixture.path("missed.json"), ctx.to_string()).unwrap();
    let output = fixture.plan(&["--at", fixture.path("missed.json").to_str().unwrap()], true);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("MISMATCH account codex: state"), "{}", stderr(&output));
    let absent = fixture.plan(&["--at", fixture.path("absent.json").to_str().unwrap()], true);
    assert_eq!(absent.status.code(), Some(1));
    assert!(stderr(&absent).contains("agents plan --at: "));
}

fn record_usage(fixture: &Fixture, written: OffsetDateTime) {
    let db = Connection::open(fixture.path("threads.db")).unwrap();
    db.execute_batch("DROP TABLE IF EXISTS usage; CREATE TABLE usage(ts TEXT, provider TEXT, account TEXT, kind TEXT, name TEXT, used_pct REAL, window_min INTEGER, resets_at TEXT, amount REAL, source TEXT)").unwrap();
    let now = OffsetDateTime::now_utc();
    let at = iso(written);
    let session_resets = iso(now + time::Duration::hours(4));
    let week_resets = iso(now + time::Duration::days(4));
    db.execute_batch(&format!(
        "INSERT INTO usage VALUES ('{at}', 'claude', 'a', 'window', 'session', 0.0, 300, '{session_resets}', NULL, 'fixture');
         INSERT INTO usage VALUES ('{at}', 'claude', 'a', 'window', 'weekly_all', 30.0, 10080, '{week_resets}', NULL, 'fixture');
         INSERT INTO usage VALUES ('{at}', 'claude', 'a', 'billing', 'hasCredits', NULL, NULL, NULL, 0.0, 'fixture');"
    ))
    .unwrap();
    for kind in ["full", "five_hour"] {
        db.execute(
            "INSERT INTO usage VALUES (?1, 'claude', 'a', 'grant', ?2, NULL, NULL, NULL, 1, 'expired manual grant')",
            rusqlite::params![iso(now - time::Duration::hours(2)), format!("cedar_ember/{kind}")],
        )
        .unwrap();
    }
}

#[test]
fn the_live_plan_prices_the_account_from_the_usage_snapshot() {
    let fixture = Fixture::new();
    fixture.orchestration(LIVE_ORCHESTRATION);
    record_usage(&fixture, OffsetDateTime::now_utc());
    let (code, plan) = fixture.json(&[]);
    assert_eq!(code, 0);
    let claude = &plan["accounts"]["claude"];
    assert_eq!(claude["used_pct"].as_f64(), Some(30.0));
    assert_eq!(claude["available_usd"].as_f64(), Some(35.0));
    let (used, pace) = (claude["used_pct"].as_f64().unwrap(), claude["pace_pct"].as_f64().unwrap());
    assert_eq!(claude["state"], if used < pace { "under pace" } else { "at or over pace" });
    assert!(!claude["resets_at"].is_null());
    assert_eq!(plan["projects"]["galileo"]["priority"], 1);
    assert_eq!(plan["projects"]["galileo"]["share"], 25);
    assert!(plan["projects"]["galileo"].get("target").is_none());
    let readout = stdout(&fixture.plan(&[], true));
    for needle in ["used 30.0%", "pace ", "available $35.00", "resets_at ", "route: claude:"] {
        assert!(readout.contains(needle), "{needle} missing from {readout}");
    }
}

#[test]
fn a_stale_usage_snapshot_fails_loudly_without_pricing() {
    let fixture = Fixture::new();
    fixture.orchestration(LIVE_ORCHESTRATION);
    record_usage(&fixture, OffsetDateTime::now_utc() - time::Duration::hours(2));
    let output = fixture.plan(&["--json"], true);
    assert_ne!(output.status.code(), Some(0));
    let text = format!("{}{}", stdout(&output), stderr(&output));
    assert!(text.contains("account claude STALE") && !text.contains("price money"), "{text}");
}

#[test]
fn a_bad_tier_or_a_missing_orchestration_file_exits_2() {
    let fixture = Fixture::new();
    fixture.orchestration("[tiers.\"claude:haiku\"]\nmodel = \"m\"\neffort = \"high\"\nstart = 101\n");
    let output = fixture.plan(&[], true);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("tier 'claude:haiku' needs provider:name, model, effort and start"));
    fs::remove_file(fixture.path("orchestration.toml")).unwrap();
    assert_eq!(fixture.plan(&[], true).status.code(), Some(2));
}
