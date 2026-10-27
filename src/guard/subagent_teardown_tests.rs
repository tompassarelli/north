use super::subagent_teardown::{refusal, transcript};
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

fn shell_start(at: &str, output: &Path) -> String {
    let started = json!({"type":"assistant","timestamp":at,"message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_sh","name":"Bash","input":{"command":"run the soak","description":"Long soak run","run_in_background":true}}]}});
    let body = format!(
        "Command running in background with ID: bshell001. Output is being written to: {}. You will be notified when it completes.",
        output.display()
    );
    let result = json!({"type":"user","timestamp":at,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_sh","content":body}]},"toolUseResult":{"stdout":"","stderr":"","backgroundTaskId":"bshell001"}});
    format!("{started}\n{result}")
}

fn task_stop(field: &str) -> String {
    json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_stop","name":"TaskStop","input":{field:"bshell001"}}]}}).to_string()
}

fn completed() -> String {
    json!({"type":"attachment","attachment":{"type":"queued_command","prompt":"<task-notification>\n<task-id>bshell001</task-id>\n<status>completed</status>\n</task-notification>"}}).to_string()
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

    fn release_output(&mut self) {
        let mut child = self.sleeper.take().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
    }

    fn fixture(&self, name: &str, lines: &str) -> String {
        let path = self.dir.path().join(format!("subagents/agent-{name}.jsonl"));
        std::fs::write(&path, format!("{lines}\n")).unwrap();
        path.display().to_string()
    }

    fn payload(&self, transcript: &str, active: bool) -> Option<String> {
        let payload = json!({"session_id":"s","hook_event_name":"SubagentStop","agent_id":"a1","agent_type":"worker","stop_hook_active":active,"transcript_path":format!("{}/main.jsonl", self.dir.path().display()),"agent_transcript_path":transcript});
        refusal(&HookInput::parse(payload.to_string().as_bytes()).unwrap())
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
fn teardown_decisions_hold() {
    let mut lane = Lane::new();
    let now = stamp(0);
    lane.hold_output();
    let start = shell_start(&now, &lane.output());
    let running = lane.fixture("running", &start);
    assert_eq!(
        lane.stop(&running).as_deref(),
        Some("You still have background tasks running: bshell001 (Long soak run). Stop each with TaskStop (long work that must outlive you runs as `job-run --notify`), then finish your report.")
    );
    assert_eq!(lane.payload(&running, true), None);
    let with = |lane: &Lane, name: &str, extra: String| lane.fixture(name, &format!("{start}\n{extra}"));
    assert_eq!(lane.stop(&with(&lane, "stopped", task_stop("task_id"))), None);
    assert_eq!(lane.stop(&with(&lane, "shellstopped", task_stop("shell_id"))), None);
    assert_eq!(lane.stop(&with(&lane, "completed", completed())), None);
    let live = lane.fixture("monitor", &monitor(&now, json!(600_000)));
    assert_eq!(
        lane.stop(&live).as_deref(),
        Some("You still have background tasks running: bmon0001 (CI watch). Stop each with TaskStop (long work that must outlive you runs as `job-run --notify`), then finish your report.")
    );
    let old = lane.fixture("expired", &monitor("2020-01-01T00:00:00.000Z", json!(600_000)));
    assert_eq!(lane.stop(&old), None);
    lane.release_output();
    assert_eq!(lane.stop(&running), None);
    let plain = json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_ls","name":"Bash","input":{"command":"ls"}}]}}).to_string();
    assert_eq!(lane.stop(&lane.fixture("plain", &plain)), None);
    let missing = format!("{}/subagents/missing.jsonl", lane.dir.path().display());
    assert_eq!(lane.stop(&missing), None);
}

#[test]
fn monitor_timeout_is_clamped_defaulted_and_malformed_lines_skipped() {
    let lane = Lane::new();
    let two_hours_ago = stamp(-7200);
    let ten_minutes_ago = stamp(-600);
    let blocked = |at: &str, timeout: Value| lane.stop(&lane.fixture("m", &monitor(at, timeout))).is_some();
    assert!(!blocked(&two_hours_ago, json!(99_999_999_999_u64)));
    assert!(!blocked(&two_hours_ago, json!(null)));
    assert!(!blocked(&ten_minutes_ago, json!("soon")));
    assert!(!blocked(&ten_minutes_ago, json!(-5)));
    assert!(blocked(&ten_minutes_ago, json!(99_999_999_999_u64)));
    assert!(blocked(&stamp(-60), json!("soon")));
    let noisy = format!("{{not json tool_use\n{}\n\"tool_use", monitor(&stamp(0), json!(60_000)));
    assert!(lane.stop(&lane.fixture("noisy", &noisy)).is_some());
}

#[test]
fn timestamps_parse_like_the_reference() {
    let now = 1_234.0;
    for (text, expected) in [
        ("2026-10-27T16:09:59.123Z", 1_793_117_399.0),
        ("1970-01-01T00:00:00", 0.0),
        ("2024-02-29T00:00:00", 1_709_164_800.0),
        ("2026-1-2T3:4:5", 1_767_323_045.0),
        ("2026-10-27T16:09:61", 1_793_117_401.0),
        ("2025-02-29T00:00:00", now),
        ("2026-13-01T00:00:00", now),
        ("0000-01-01T00:00:00", now),
        ("2026-10-27 16:09:59", now),
        ("2026-10-27T16:09", now),
        ("", now),
    ] {
        assert_eq!(transcript::epoch(text, now), expected, "{text}");
    }
}

#[test]
fn transcript_lines_are_bounded_and_scanned_in_a_stream() {
    let long = "x".repeat(17 << 20);
    let notice = completed();
    let body = format!("{long}\n{}\n{long}{notice}\n", monitor(&stamp(0), json!(60_000)));
    let scan = transcript::scan_reader(body.as_bytes(), 0.0).unwrap();
    assert_eq!(scan.tasks.len(), 1);
    assert!(scan.ended.is_empty());
    let first = format!("{notice}\n{long}");
    assert!(transcript::scan_reader(first.as_bytes(), 0.0).unwrap().ended.contains("bshell001"));
    let tail = format!("{}\n{notice}", "y".repeat(100));
    assert!(transcript::scan_reader(tail.as_bytes(), 0.0).unwrap().ended.contains("bshell001"));
}
