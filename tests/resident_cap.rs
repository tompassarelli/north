use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::json;

const CLAUDE: &str = "/home/tom/.local/lib/claude/current/bin/claude";
const CODEX: &str = "/home/tom/.local/lib/codex/current/bin/codex";
const HOLD: &str = "Hold: nexus has 6/6 resident agent CLIs; place this worker on laptop|cloud\n";

struct Scenario {
    root: tempfile::TempDir,
}

fn proc(base: &Path, pid: u32, ppid: u32, comm: &str, argv: &[&str]) {
    let dir = base.join(pid.to_string());
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("stat"), format!("{pid} ({comm}) S {ppid} {pid} {pid} 0 -1\n")).unwrap();
    fs::write(dir.join("cmdline"), argv.iter().map(|word| format!("{word}\0")).collect::<String>()).unwrap();
}

impl Scenario {
    fn new(registered: u32, unregistered: u32) -> Self {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("proc");
        proc(&base, 1, 0, "systemd", &["/run/current-system/systemd/lib/systemd/systemd"]);
        let mut sessions = vec![json!({"name": "done", "status": "exited", "host_pid": 1})];
        for i in 0..registered {
            let (shell, agent) = (1000 + 10 * i, 1001 + 10 * i);
            proc(&base, shell, 1, "bash", &["bash", "-c", "north fleet launch --name test; claude"]);
            if i % 2 == 1 {
                proc(&base, agent, shell, "codex", &[CODEX, "resume", "--remote", "ws://x"]);
            } else {
                proc(&base, agent, shell, "claude", &[CLAUDE, "-p", "--input-format", "stream-json"]);
            }
            proc(&base, agent + 1, agent, "bash", &["/run/current-system/sw/bin/bash", "-c", "rg x"]);
            sessions.push(json!({"name": format!("lead-{i}"), "status": "working", "host_pid": shell}));
        }
        for i in 0..unregistered {
            proc(&base, 3000 + i, 1, "claude", &[CLAUDE, "--resume"]);
        }
        proc(&base, 4000, 1, "codex", &[CODEX, "app-server", "--listen", "ws://x"]);
        proc(&base, 4001, 1, "claude", &[CLAUDE, "-p", "one shot"]);
        fs::write(root.path().join("fleet.json"), serde_json::to_string(&sessions).unwrap()).unwrap();
        let north = root.path().join("north");
        fs::write(&north, "#!/bin/sh\n[ \"$*\" = \"fleet list --json\" ] || exit 2\ncat \"$(dirname \"$0\")/fleet.json\"\n").unwrap();
        fs::set_permissions(&north, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(root.path().join("orchestration.toml"), "[hosts.nexus]\nresident_cap = 6\n").unwrap();
        Self { root }
    }

    fn behavior(&self) -> PathBuf {
        self.root.path().join("behavior")
    }

    fn run(&self, host: &str, arguments: &[&str], extra: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_north"));
        command
            .args(["resident-cap"])
            .args(arguments)
            .env_remove("CASE")
            .env_remove("FACT")
            .env_remove("RESIDENT_CAP_PID")
            .env("RESIDENT_CAP_PROC", self.root.path().join("proc"))
            .env("RESIDENT_CAP_HOST", host)
            .env("NORTH_BIN", self.root.path().join("north"))
            .env("AGENTS_ORCHESTRATION", self.root.path().join("orchestration.toml"))
            .env("CODEX_BEHAVIOR_STATE", self.behavior());
        extra.iter().for_each(|(name, value)| {
            command.env(name, value);
        });
        command.output().unwrap()
    }

    fn gate(&self, host: &str, provider: &str, words: &[&str]) -> (i32, String) {
        let mut arguments = vec!["gate", provider];
        arguments.extend(words);
        let output = self.run(host, &arguments, &[]);
        (output.status.code().unwrap(), String::from_utf8(output.stderr).unwrap())
    }
}

#[test]
fn a_seventh_resident_launch_is_held_with_the_hold_line() {
    let six = Scenario::new(6, 0);
    assert_eq!(six.gate("nexus", "claude", &["--name", "worker-7", "Read brief.md"]), (3, HOLD.into()));
    assert_eq!(six.gate("nexus", "codex", &["resume", "-m", "gpt-6.1-sol", "read brief.md"]), (3, HOLD.into()));
    assert_eq!(Scenario::new(5, 0).gate("nexus", "claude", &["--name", "worker-6"]), (0, String::new()));
}

#[test]
fn an_unregistered_top_level_cli_counts_toward_the_cap() {
    assert_eq!(Scenario::new(5, 1).gate("nexus", "claude", &[]), (3, HOLD.into()));
}

#[test]
fn cloud_launches_and_hosts_without_a_cap_are_never_held() {
    let six = Scenario::new(6, 0);
    assert_eq!(six.gate("nexus", "claude", &["--cloud", "Read brief.md"]), (0, String::new()));
    assert_eq!(six.gate("whiterabbit", "claude", &[]), (0, String::new()));
}

#[test]
fn an_urgent_case_with_a_fact_passes_and_is_logged_beside_the_spawn_gate_overrides() {
    let six = Scenario::new(6, 0);
    let fact = "lead restart after crash, cannot wait";
    let output = six.run("nexus", &["gate", "claude"], &[("CASE", "URGENT"), ("FACT", fact)]);
    assert_eq!((output.status.code(), output.stderr.as_slice()), (Some(0), &b""[..]));
    let log = fs::read_to_string(six.behavior().join("verify-overrides.jsonl")).unwrap();
    assert!(log.contains("\"gate\": \"resident-cap\""), "{log}");
    let entry: serde_json::Value = serde_json::from_str(log.trim()).unwrap();
    assert_eq!((entry["gate"].as_str(), entry["host"].as_str(), entry["fact"].as_str()), (Some("resident-cap"), Some("nexus"), Some(fact)));
    let short = six.run("nexus", &["gate", "claude"], &[("CASE", "URGENT"), ("FACT", "too short")]);
    assert_eq!(short.status.code(), Some(3));
}

#[test]
fn status_prints_the_live_count_and_the_cap() {
    let six = Scenario::new(6, 0);
    assert_eq!(String::from_utf8(six.run("nexus", &["status"], &[]).stdout).unwrap(), "nexus: 6/6 resident agent CLIs\n");
    assert_eq!(String::from_utf8(six.run("laptop", &["status"], &[]).stdout).unwrap(), "laptop: 6 resident agent CLIs, no cap\n");
    assert_eq!(six.run("nexus", &["bogus"], &[]).status.code(), Some(2));
}
