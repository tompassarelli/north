use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rusqlite::Connection;
use tempfile::TempDir;

struct Fixture {
    dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn agents(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_north"))
            .arg("agents")
            .args(args)
            .env("NORTH_HOME", env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap()
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn bad_plan_flags_print_the_usage_and_exit_2() {
    let fixture = Fixture::new();
    let output = fixture.agents(&["plan", "--bogus"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage: agents {status"));
    let scored = fixture.agents(&["plan", "--score", "north", "--quote"]);
    assert_eq!(scored.status.code(), Some(2));
    let unknown = fixture.agents(&["nonsense"]);
    assert_eq!(unknown.status.code(), Some(2));
}

fn quote(fixture: &Fixture, project: &str) -> Vec<String> {
    let home = fixture.path("home");
    fs::create_dir_all(home.join(".local/state/agents")).unwrap();
    fs::write(
        fixture.path("probe.json"),
        r#"{"committedBatchCpus": 0, "aggregateCpuLimit": 20, "protectedCpuSomeAvg10": 0}"#,
    )
    .unwrap();
    let gate = r#"{"epoch": 9999999999, "stale_min": 30, "refusal": null, "paces": {}}"#;
    fs::write(
        home.join(".local/state/agents/usage-gate.json"),
        format!(r#"{{"claude": {gate}, "codex": {gate}}}"#),
    )
    .unwrap();
    let inherited = std::env::var("PATH").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args([
            "agents",
            "plan",
            "--quote",
            "--project",
            project,
            "--boxes",
            "4",
        ])
        .env("PATH", inherited)
        .env("NORTH_HOME", env!("CARGO_MANIFEST_DIR"))
        .env("AGENTS_ORCHESTRATION", fixture.path("quote.toml"))
        .env("THREADS_DB", fixture.path("snapshot.db"))
        .env("HOME", &home)
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("SPAWN_CAPACITY_STATUS", fixture.path("probe.json"))
        .env("SPAWN_CAPACITY_HOME", &home)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_project_quote_takes_the_critical_path_at_the_gate_width_with_one_dated_decision() {
    let fixture = Fixture::new();
    fs::write(
        fixture.path("quote.toml"),
        "[projects.north]\ntarget = \"2026-10-13T09:30:00Z\"\n[projects.wisp]\n",
    )
    .unwrap();
    let db = Connection::open(fixture.path("snapshot.db")).unwrap();
    db.execute_batch("CREATE TABLE runs(agent TEXT, item TEXT, outcome TEXT, role TEXT, staffed TEXT, started TEXT, ended TEXT, landed_at TEXT, cost_usd REAL, expects_landing INTEGER, landed INTEGER)").unwrap();
    for project in ["north", "wisp"] {
        for (number, end, cost) in [
            (1, "2026-10-12T06:20:00Z", 1.0),
            (2, "2026-10-12T06:40:00Z", 3.0),
        ] {
            db.execute("INSERT INTO runs VALUES (?1, ?2, 'done', 'worker', '2026-10-12T06:00:00Z', '2026-10-12T06:00:00Z', ?3, ?3, ?4, 1, 1)", rusqlite::params![format!("{project}-{number}"), format!("{project}#{number}"), end, cost]).unwrap();
        }
    }
    drop(db);
    let dated = quote(&fixture, "north");
    let undated = quote(&fixture, "wisp");
    let decision = |lines: &[String]| {
        lines
            .iter()
            .filter(|line| line.starts_with("Blocked: decision for Tom:"))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert!(decision(&undated).is_empty(), "{undated:?}");
    let decisions = decision(&dated);
    assert_eq!(decisions.len(), 1, "{dated:?}");
    let rest = decisions[0]
        .strip_prefix("Blocked: decision for Tom: north date ")
        .unwrap();
    let (date, tail) = rest.split_once(' ').unwrap();
    assert_eq!(tail, "$8.00 [approve|later date|smaller scope]");
    let epoch = |when: &str| {
        let output = Command::new("date")
            .args(["-u", "-d", when, "+%s"])
            .output()
            .unwrap();
        String::from_utf8(output.stdout)
            .unwrap()
            .trim()
            .parse::<i64>()
            .unwrap()
    };
    let minutes = (epoch(date) - epoch("now")) as f64 / 60.0;
    assert!((29.0..=30.0).contains(&minutes), "{minutes}");
    assert!(
        ["minutes/box 30", "width 4", "concurrency 2"]
            .iter()
            .all(|want| dated[0].contains(want)),
        "{dated:?}"
    );
}

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn clone_of_fresh_origin(root: &Path, name: &str) {
    let origin = root.join(format!("{name}-origin"));
    fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "-q", "-b", "main"]);
    git(&origin, &["commit", "-q", "--allow-empty", "-m", "base"]);
    git(root, &["clone", "-q", &format!("{name}-origin"), name]);
}

#[test]
fn sync_publishes_from_local_main_when_the_fetch_fails_and_the_policy_gate_passes() {
    let fixture = Fixture::new();
    let root = fixture.path("sync");
    for name in ["south", "north"] {
        clone_of_fresh_origin(&root, name);
    }
    git(
        &root.join("south-origin"),
        &["commit", "-q", "--allow-empty", "-m", "remote"],
    );
    git(
        &root.join("south"),
        &[
            "remote",
            "set-url",
            "origin",
            root.join("gone").to_str().unwrap(),
        ],
    );
    let agents = root.join("south/agents");
    fs::create_dir_all(&agents).unwrap();
    fs::write(
        agents.join("catalog-config.json"),
        r#"{"activation": {}, "registrations": {}, "baselines": []}"#,
    )
    .unwrap();
    fs::write(agents.join("orchestration.toml"), "[projects]\n").unwrap();
    fs::write(agents.join("AGENTS.md"), "# Policy\n").unwrap();
    let sha = String::from_utf8(
        Command::new("sha256sum")
            .arg(agents.join("AGENTS.md"))
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let sha = sha.split_whitespace().next().unwrap().to_owned();
    fs::write(agents.join("operating-point.toml"), format!("date = \"d\"\nmodel = \"m\"\npolicy_sha256 = \"{sha}\"\nstartup_tokens = 1000000\nconformance = 1.0\nn = 1\n")).unwrap();
    fs::create_dir_all(root.join("replay")).unwrap();
    fs::write(
        root.join("replay/sweep-d.jsonl"),
        format!(
            "{{\"policy_sha256\": \"{sha}\", \"model\": \"m\", \"conformance\": 1.0, \"n\": 1}}\n"
        ),
    )
    .unwrap();
    let before = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(root.join("south"))
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let home = fixture.path("home");
    fs::create_dir_all(&home).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_north"))
        .args(["agents", "sync"])
        .env("HOME", &home)
        .env("NORTH_HOME", env!("CARGO_MANIFEST_DIR"))
        .env("AGENTS_OPERATOR_DIR", &agents)
        .env("AGENTS_REPLAY_DIR", root.join("replay"))
        .env(
            "NORTH_REPO_ROOTS",
            format!(
                r#"{{"south":"{0}/south","north":"{0}/north","nixos-config":"{0}/none"}}"#,
                root.display()
            ),
        )
        .output()
        .unwrap();
    let said = text(&output);
    assert!(
        said.contains("agents sync: south fetch failed; using local main"),
        "{said}"
    );
    assert!(!said.contains("generation not published"), "{said}");
    let after = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(root.join("south"))
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(before, after);
    assert!(said.contains("current/activation.json"), "{said}");
}
