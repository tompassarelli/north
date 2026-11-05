#![cfg(test)]

use super::subagent_teardown::refusal_in;
use crate::hook::HookInput;
use serde_json::{Value, json};
use std::path::Path;
use std::process::{Child, Command, Stdio};

fn stamp(offset: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + offset;
    let (year, month, day) = civil(now.div_euclid(86_400));
    let rest = now.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.000Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

fn shell_command(at: &str, output: &Path, command: &str) -> String {
    shell_task(at, output, command, "Log watch")
}

fn shell_task(at: &str, output: &Path, command: &str, description: &str) -> String {
    let started = json!({"type":"assistant","timestamp":at,"message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_sh","name":"Bash","input":{"command":command,"description":description,"run_in_background":true}}]}});
    let body = format!(
        "Command running in background with ID: bshell001. Output is being written to: {}. You will be notified when it completes.",
        output.display()
    );
    let result = json!({"type":"user","timestamp":at,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_sh","content":body}]},"toolUseResult":{"stdout":"","stderr":"","backgroundTaskId":"bshell001"}});
    format!("{started}\n{result}")
}

fn monitor(at: &str, timeout: Value) -> String {
    let started = json!({"type":"assistant","timestamp":at,"message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_mon","name":"Monitor","input":{"command":"tail -f x","description":"CI watch","timeout_ms":timeout}}]}});
    let result = json!({"type":"user","timestamp":at,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_mon","content":"Monitor started (task bmon0001, expires in 10m)."}]},"toolUseResult":{"taskId":"bmon0001","timeoutMs":600000,"persistent":false}});
    format!("{started}\n{result}")
}

struct Lane {
    dir: tempfile::TempDir,
    sleeper: Option<Child>,
}

impl Lane {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("subagents")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        Self { dir, sleeper: None }
    }

    fn output(&self) -> std::path::PathBuf {
        self.dir.path().join("tasks/bshell001.output")
    }

    fn hold_output(&mut self) {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.output())
            .unwrap();
        self.sleeper = Some(
            Command::new("sleep")
                .arg("60")
                .stdout(Stdio::from(file))
                .spawn()
                .unwrap(),
        );
    }

    fn fixture(&self, name: &str, lines: &str) -> String {
        let path = self.dir.path().join(format!("subagents/agent-{name}.jsonl"));
        std::fs::write(&path, format!("{lines}\n")).unwrap();
        path.display().to_string()
    }

    fn payload(&self, transcript: &str, active: bool) -> Option<String> {
        self.payload_in(transcript, active, true)
    }

    fn payload_in(&self, transcript: &str, active: bool, fleet: bool) -> Option<String> {
        let payload = json!({"session_id":"s","hook_event_name":"SubagentStop","agent_id":"a1","agent_type":"worker","stop_hook_active":active,"transcript_path":format!("{}/main.jsonl", self.dir.path().display()),"agent_transcript_path":transcript});
        refusal_in(&HookInput::parse(payload.to_string().as_bytes()).unwrap(), fleet)
    }

    fn stop(&self, transcript: &str) -> Option<String> {
        self.payload(transcript, false)
    }
}

impl Drop for Lane {
    fn drop(&mut self) {
        if let Some(mut child) = self.sleeper.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[test]
fn builds_tests_and_landings_may_outlive_the_turn_and_only_endless_watchers_refuse() {
    let mut lane = Lane::new();
    let now = stamp(0);
    lane.hold_output();
    let held = |command: &str| lane.fixture("cmd", &shell_command(&now, &lane.output(), command));
    for command in [
        "safe-push --to main",
        "cd ~/code/north/worktrees/x && bin/safe-push --to main",
        "FOO=1 safe-push --to main; echo done",
        "cargo test --locked",
        "nix develop -c cargo build --release 2>&1 | tail -20",
        "cargo clippy --all-targets -- -D warnings > clippy.log 2>&1",
        "bun test",
        "run the soak",
        "gh run watch 123",
        "until test -f done; do sleep 5; done",
        "while true; do test -f done && break; sleep 5; done",
        "tail --pid=4242 -f build.log",
    ] {
        assert_eq!(lane.stop(&held(command)), None, "{command}");
        assert_eq!(lane.payload_in(&held(command), false, false), None, "{command}");
    }
    for command in [
        "tail -f build.log",
        "tail -f build.log | grep -m1 Finished",
        "tail -n 50 -F run.log",
        "journalctl --user -fu north-watch",
        "watch -n 5 git status",
        "cargo watch -x test",
        "inotifywait -m src",
        "while true; do date; sleep 5; done",
        "while :; do cat status; sleep 1; done",
    ] {
        assert!(lane.stop(&held(command)).is_some(), "{command}");
    }
    let live = lane.fixture("monitor", &monitor(&now, json!(600_000)));
    assert!(lane.stop(&live).is_some());
    let both = format!("{}\n{}", monitor(&now, json!(600_000)), shell_command(&now, &lane.output(), "cargo build --release"));
    let refused = lane.stop(&lane.fixture("both", &both)).unwrap();
    assert!(refused.contains("bmon0001") && !refused.contains("bshell001"), "{refused}");
}

