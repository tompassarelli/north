use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const POINT: &str = "date = \"2026-10-12T07:19:38Z\"\nmodel = \"m\"\nvariant = \"base\"\npolicy_sha256 = \"old\"\nstartup_tokens = 5213\nconformance = 0.5\nn = 4\nevidence = \"e\"\nweak_scenarios = [\"b\"]\n# audit note kept by promote\n";
const ATTACHED: &str = include_str!("../harness/scripts/fixtures/haiku-opus-advisor.debug-advisor-lines.log");
const OK_CLAUDE: &str = "#!/bin/sh\n[ \"$1\" = --version ] && { echo 9.9; exit 0; }\necho '{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"ok\",\"model\":\"m\"}'\n";

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new(claude: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = root.path();
        let replay = path.join("agents/replay");
        fs::create_dir_all(replay.join("coding")).unwrap();
        fs::create_dir_all(path.join("bin")).unwrap();
        fs::create_dir_all(path.join("home")).unwrap();
        fs::write(path.join("agents/AGENTS.md"), "# Policy\n\n## Rules\n\n- Keep it short.\n").unwrap();
        fs::write(path.join("agents/catalog-config.json"), r#"{"activation": {}, "registrations": {}, "baselines": []}"#).unwrap();
        fs::write(path.join("agents/orchestration.toml"), "[projects]\n").unwrap();
        fs::write(path.join("agents/operating-point.toml"), POINT).unwrap();
        fs::write(replay.join("variants.toml"), "[[variant]]\nname = \"base\"\n").unwrap();
        for id in ["a", "b"] {
            fs::write(replay.join(format!("{id}.json")), format!(r#"{{"id": "{id}", "prompt": "p", "detector": {{"kind": "regex", "pattern": "ok"}}}}"#)).unwrap();
        }
        let fixture = Self { root };
        fixture.claude(claude);
        fixture
    }

    fn claude(&self, script: &str) {
        let file = self.root.path().join("bin/claude");
        north_core::foundation::shim::write_executable(&file, script);
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.root.path().join(rel)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command
            .arg("policy-replay")
            .env_clear()
            .env("PATH", format!("{}:{}", self.path("bin").display(), std::env::var("PATH").unwrap_or_default()))
            .env("HOME", self.path("home"))
            .env("NORTH_FIRN", self.root.path());
        command
    }

    fn run(&self, task: &Path, out: &Path, extra: &[&str], env: &[(&str, &Path)]) -> Output {
        let mut command = self.command();
        command.arg("run").arg(task).args(["--arm", "full", "--out"]).arg(out).args(extra);
        for (name, value) in env {
            command.env(name, value);
        }
        command.output().unwrap()
    }

    fn rows(&self, file: &Path) -> Vec<serde_json::Value> {
        fs::read_to_string(file).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect()
    }
}

fn survivors(group: &str) -> String {
    let found = Command::new("pgrep").args(["-g", group]).output().unwrap();
    String::from_utf8_lossy(&found.stdout).into_owned()
}

fn group_claude(pgid_file: &Path) -> String {
    format!(
        "#!/bin/sh\n[ \"$1\" = --version ] && {{ echo 9.9; exit 0; }}\ntrap '' TERM\n(trap '' TERM; exec sleep 100) &\nps -o pgid= -p $$ | tr -d ' ' > {0}.tmp\nmv {0}.tmp {0}\nsleep 100\n",
        pgid_file.display()
    )
}

#[test]
fn a_sweep_writes_only_the_candidate_and_leaves_the_operating_point_unchanged() {
    let fixture = Fixture::new(OK_CLAUDE);
    let (candidate, out) = (fixture.path("candidate.toml"), fixture.path("sweep.jsonl"));
    let swept = fixture
        .command()
        .arg("sweep")
        .args([fixture.path("agents/replay/a.json"), fixture.path("agents/replay/b.json")])
        .arg("--variants")
        .arg(fixture.path("agents/replay/variants.toml"))
        .arg("--candidate")
        .arg(&candidate)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(swept.status.success(), "{}", String::from_utf8_lossy(&swept.stderr));
    assert_eq!(fs::read_to_string(fixture.path("agents/operating-point.toml")).unwrap(), POINT);
    let text = fs::read_to_string(&candidate).unwrap();
    assert!(text.contains("conformance = 1.0") && text.contains("\"a\" = [1, 1]") && text.contains("\"b\" = [1, 1]"), "{text}");
}

#[test]
fn a_timed_out_replay_kills_and_reaps_the_whole_group_including_a_child_ignoring_sigterm() {
    let fixture = Fixture::new("");
    let group = fixture.path("group.pgid");
    fixture.claude(&group_claude(&group));
    let timed = fixture.path("agents/replay/timed.json");
    fs::write(&timed, r#"{"id": "timed", "prompt": "p", "timeout_s": 1, "detector": {"kind": "regex", "pattern": "ok"}}"#).unwrap();
    let out = fixture.path("timed.jsonl");
    let stopped = fixture.run(&timed, &out, &[], &[]);
    let row = fixture.rows(&out).remove(0);
    assert_eq!(stopped.status.code(), Some(1), "{stopped:?}");
    assert!(row["pass"].is_null() && row["error"].as_str().unwrap().contains("timeout"), "{row}");
    assert_eq!(survivors(fs::read_to_string(&group).unwrap().trim()), "");
}

#[test]
fn a_replay_signalled_with_sigterm_exits_143_and_leaves_no_group_member() {
    let fixture = Fixture::new("");
    let ready = fixture.path("group.pgid");
    fixture.claude(&group_claude(&ready));
    let mut child = fixture.command().arg("run").arg(fixture.path("agents/replay/a.json")).args(["--arm", "full", "--out"]).arg(fixture.path("signal.jsonl")).spawn().unwrap();
    for _ in 0..200 {
        if ready.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let pgid = fs::read_to_string(&ready).unwrap();
    assert!(Command::new("kill").arg("-TERM").arg(child.id().to_string()).status().unwrap().success());
    assert_eq!(child.wait().unwrap().code(), Some(143));
    assert_eq!(survivors(pgid.trim()), "");
}

#[test]
fn a_command_detector_passes_on_exit_zero_fails_otherwise_and_fails_on_its_timeout() {
    let fixture = Fixture::new(OK_CLAUDE);
    let cases = [("built", "test -f built", 1.0, true), ("failing", "exit 2", 1.0, false), ("slow", "sleep 5", 0.5, false)];
    for (id, command, timeout, expected) in cases {
        let task = fixture.path(&format!("agents/replay/{id}.json"));
        let setup = if id == "built" { r#","setup": {"built": ""}"# } else { "" };
        fs::write(&task, format!(r#"{{"id": "{id}", "prompt": "p", "detector": {{"kind": "command", "command": "{command}", "timeout_s": {timeout}}}{setup}}}"#)).unwrap();
        let out = fixture.path(&format!("{id}.jsonl"));
        let ran = fixture.run(&task, &out, &[], &[]);
        assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
        assert_eq!(fixture.rows(&out)[0]["pass"], expected, "{id}");
    }
}

#[test]
fn a_coding_scenario_with_its_own_policy_runs_model_effort_advisor_and_nulls_a_detached_advisor() {
    let fixture = Fixture::new("");
    let argv = fixture.path("argv");
    let off = fixture.path("advisor_off");
    fixture.claude(&format!(
        "#!/bin/sh\n[ \"$1\" = --version ] && {{ echo 9.9; exit 0; }}\necho \"$@\" > {}\necho \"${{CLAUDE_CODE_DISABLE_ADVISOR_TOOL:-}}\" > {}\nwhile [ $# -gt 0 ]; do [ \"$1\" = --debug-file ] && [ -n \"$STUB_DEBUG\" ] && cp \"$STUB_DEBUG\" \"$2\"; shift; done\necho done > out.txt\necho '{{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"ok\",\"model\":\"m\"}}'\n",
        argv.display(),
        off.display()
    ));
    let task = fixture.path("agents/replay/coding/t.json");
    fs::write(&task, r#"{"id": "t", "prompt": "p", "policy": "../../AGENTS.md", "detector": {"kind": "command", "command": "grep -q done out.txt"}}"#).unwrap();
    let attached = fixture.path("attached.log");
    let detached = fixture.path("detached.log");
    fs::write(&attached, ATTACHED).unwrap();
    fs::write(&detached, ATTACHED.lines().filter(|line| !line.contains("Server-side tool enabled")).collect::<Vec<_>>().join("\n")).unwrap();
    let go = |name: &str, extra: &[&str], log: &Path| {
        let out = fixture.path(name);
        (fixture.run(&task, &out, extra, &[("STUB_DEBUG", log)]), out)
    };

    let (ran, out) = go("x.jsonl", &["--model", "haiku", "--effort", "low", "--advisor", "opus"], &attached);
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    let row = fixture.rows(&out).remove(0);
    let line = fs::read_to_string(&argv).unwrap();
    for needed in ["--model haiku", "--effort low", "--advisor opus", "--strict-mcp-config"] {
        assert!(line.contains(needed), "{line}");
    }
    assert_eq!(row["advisor_attached"], true);
    assert_eq!(row["advisor_model_resolved"], "claude-opus-5-5");
    assert_eq!((&row["pass"], &row["model"], &row["effort"], &row["advisor"], &row["advisor_calls"]), (&true.into(), &"haiku".into(), &"low".into(), &"opus".into(), &0.into()));
    assert!(row["usd"].as_f64() == Some(0.0) && row["usd_basis"].as_str().unwrap().starts_with("total_cost_usd"), "{row}");

    let (_, out) = go("d.jsonl", &["--model", "haiku", "--advisor", "opus"], &detached);
    let row = fixture.rows(&out).remove(0);
    assert!(row["pass"].is_null() && row["advisor_attached"] == false && row["error"].as_str().unwrap().contains("not attached"), "{row}");
    assert_eq!(fs::read_to_string(&off).unwrap().trim(), "");

    let (ran, out) = go("c.jsonl", &["--model", "haiku"], &detached);
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    assert_eq!(fs::read_to_string(&off).unwrap().trim(), "1");
    assert_eq!(fixture.rows(&out)[0]["pass"], true);

    let (_, out) = go("l.jsonl", &["--model", "haiku"], &attached);
    let row = fixture.rows(&out).remove(0);
    assert!(row["pass"].is_null() && row["error"].as_str().unwrap().contains("control run had an advisor"), "{row}");

    let (fable, _) = go("y.jsonl", &["--advisor", "fable"], &detached);
    assert!(!fable.status.success() && String::from_utf8_lossy(&fable.stderr).contains("Fable"), "{fable:?}");
}

#[test]
fn a_replay_launches_the_installed_claude_runtime_over_a_path_entry() {
    let fixture = Fixture::new("#!/bin/sh\nexit 3\n");
    let installed = fixture.path("home/.local/lib/claude/current/bin/claude");
    fs::create_dir_all(installed.parent().unwrap()).unwrap();
    north_core::foundation::shim::write_executable(&installed, OK_CLAUDE);
    let out = fixture.path("installed.jsonl");
    let ran = fixture.run(&fixture.path("agents/replay/a.json"), &out, &[], &[]);
    assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
    let row = fixture.rows(&out).remove(0);
    assert_eq!(row["pass"], true, "{row}");
    assert_eq!(row["claude_version"], "9.9", "{row}");
}
