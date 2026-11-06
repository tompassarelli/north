use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use super::*;
use serde_json::json;
use crate::fleet::claims::Held;
use crate::fleet::books::{Books, Landing, SessionSpend};
use crate::fleet::model::{Delivery, Ending};
use crate::fleet::gates::{Closure, Gate, GateBook, GateRequest};
use crate::fleet::registry::TurnWatch;
use crate::fleet::receipts::{Courier, ReceiptBook};
use crate::fleet::turns::{RunRecord, RunSink};
use crate::fleet::session_host::{SessionHost, Watcher};

#[derive(Default)]
struct Recorded(Mutex<Vec<RunRecord>>, Mutex<Vec<(String, String, Delivery)>>, Mutex<Vec<GateRequest>>, Mutex<Vec<(PathBuf, String)>>);

impl RunSink for Recorded {
    fn record_run(&self, run: &RunRecord) -> NorthResult<u64> {
        let mut runs = self.0.lock().unwrap();
        runs.push(run.clone());
        Ok(runs.iter().filter(|recorded| recorded.session == run.session).count() as u64)
    }

    fn join_task(&self, _task_ref: &str, _session: &str) -> NorthResult<()> {
        Ok(())
    }
}

impl ReceiptBook for Recorded {
    fn claim_receipt(&self, _session: &str, _key: &str, _claimed_ms: u64) -> NorthResult<bool> {
        Ok(true)
    }
}

impl Courier for Recorded {
    fn notify(&self, parent: &str, text: String, delivery: Delivery) {
        self.1.lock().unwrap().push((parent.to_owned(), text, delivery));
    }

    fn ask(&self, request: GateRequest) {
        self.2.lock().unwrap().push(request);
    }

    fn answer(&self, host_dir: &Path, frame: String) {
        self.3.lock().unwrap().push((host_dir.to_path_buf(), frame));
    }
}

impl GateBook for Recorded {
    fn open_gate(&self, _request: &GateRequest, _opened_ms: u64) -> NorthResult<i64> {
        Ok(0)
    }

    fn gate(&self, _id: i64) -> NorthResult<Option<Gate>> {
        Ok(None)
    }

    fn gates(&self, _closed_since_ms: u64) -> NorthResult<Vec<Gate>> {
        Ok(Vec::new())
    }

    fn close_gate(&self, _id: i64, _closure: &Closure) -> NorthResult<bool> {
        Ok(false)
    }
}

impl Books for Recorded {
    fn spend(&self) -> NorthResult<Vec<SessionSpend>> {
        Ok(Vec::new())
    }

    fn landed_since(&self, _since_ms: u64) -> NorthResult<Vec<Landing>> {
        Ok(Vec::new())
    }

    fn record_landing(&self, _task_ref: &str, _sha: &str, _landed_ms: u64, _wall_ms: Option<u64>) -> NorthResult<bool> {
        Ok(false)
    }
}

fn silent_codex_turn(root: &Path, wall_ms: u64) -> HostConfig {
    let thread = r#""threadId":"th-1""#;
    let script = format!(
        r#"echo '{{"method":"turn/started","params":{{{thread},"turn":{{"id":"turn-1"}}}}}}'; read -r _; echo '{{"method":"turn/completed","params":{{{thread},"turn":{{"id":"turn-1","status":"completed","durationMs":{wall_ms}}}}}}}'; exec sleep 60"#
    );
    HostConfig {
        name: "codex-test".into(),
        generation: 1,
        state_dir: root.to_path_buf(),
        socket: None,
        idle_exit: None,
        provider: Some("codex".into()),
        runtime: vec!["sh".into(), "-c".into(), script],
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn notifications_after_the_turn_do_not_keep_a_codex_host_awake() {
    let root = tempfile::tempdir().unwrap();
    let thread = r#""threadId":"th-1""#;
    let script = format!(
        r#"echo '{{"method":"turn/started","params":{{{thread},"turn":{{"id":"turn-1"}}}}}}'; echo '{{"method":"turn/completed","params":{{{thread},"turn":{{"id":"turn-1","status":"completed"}}}}}}'; while :; do echo '{{"method":"skills/changed","params":{{}}}}'; sleep 0.2; done"#
    );
    let mut config = silent_codex_turn(root.path(), 0);
    config.runtime = vec!["sh".into(), "-c".into(), script];
    let watch = TurnWatch::new(Arc::new(Codex), Arc::new(Recorded::default()), Arc::new(Recorded::default()), Arc::new(Held::default()), root.path().to_path_buf(), root.path().to_path_buf());
    let mut host = SessionHost::spawn_watched(&config, Watcher::new(Box::new(watch))).unwrap();
    host.rest_when_idle(Some(Duration::from_secs(1)));
    let parked = tokio::time::timeout(Duration::from_secs(10), host.wait()).await;
    assert_eq!(parked.ok().map(Result::unwrap), Some(143), "an idle host must park despite steady non-turn output");
}

#[test]
fn a_launch_inherits_the_calling_session_as_parent_else_the_proxy_unless_told_none() {
    let providers = [ProviderName::new("claude")];
    let test_request = launch_request(&providers, &["--provider", "claude", "--account", "c", "--test", "test prompt"], None).unwrap();
    assert!(test_request.verification);
    let parent = |flags: &[&str], caller: Option<&str>| {
        let words = [&["--provider", "claude", "--account", "c", "--name", "w"], flags, &["go"]].concat();
        launch_request(&providers, &words, caller.map(str::to_owned)).unwrap().parent
    };
    assert_eq!(parent(&[], Some("boss")), Some("boss".into()));
    assert_eq!(parent(&["--parent", "none"], Some("boss")), None);
    assert_eq!(parent(&["--parent", "lead"], Some("boss")), Some("lead".into()));
    assert_eq!(parent(&[], None), Some("proxy".into()));
    assert_eq!(parent(&["--parent", "none"], None), None);
    let proxy = launch_request(&providers, &["--provider", "claude", "--account", "c", "--name", "proxy", "go"], None).unwrap();
    assert_eq!(proxy.parent, None, "the proxy is never its own parent");
    let wait = |flags: &[&str], caller: Option<&str>| launch_parts(&providers, &[&["--provider", "claude", "--account", "c"], flags, &["go"]].concat(), caller.map(str::to_owned), true).unwrap().wait;
    assert_eq!(wait(&[], None), Some(false), "a launch from outside the fleet does not hold for admission under its default proxy parent");
    assert_eq!((wait(&[], Some("boss")), wait(&["--parent", "lead"], None), wait(&["--wait"], None)), (None, None, Some(true)));
}

#[test]
fn a_launch_flag_never_takes_the_next_flag_as_its_value() {
    let providers = [ProviderName::new("claude")];
    let words = ["--provider", "claude", "--account", "c", "--local", "--name", "firn-statix", "go"];
    assert!(launch_request(&providers, &words, None).is_err());
}

fn seconds(rollover: u64, finished_exit: u64) -> crate::fleet::idle::Policy {
    crate::fleet::idle::Policy { eta_minute: Duration::from_secs(60), rollover_after: Duration::from_secs(rollover), cache_lifetime: Duration::from_secs(60), rollover_min_tokens: 200_000, finished_exit_after: Duration::from_secs(finished_exit), registered_park_after: Duration::from_secs(30) }
}

fn claude_turn(root: &Path, context: u64, reply: &str) -> (HostConfig, PathBuf) {
    let usage = json!({"input_tokens": 1, "output_tokens": 5, "cache_read_input_tokens": context - 1, "cache_creation_input_tokens": 0});
    let assistant = json!({"type": "assistant", "session_id": "s", "parent_tool_use_id": null, "message": {"id": "m1", "content": [{"type": "text", "text": reply}], "usage": usage}});
    let result = json!({"type": "result", "subtype": "success", "session_id": "s", "is_error": false});
    let stdin = root.join("stdin.jsonl");
    let script = format!("printf '%s\\n' '{assistant}' '{result}'; while read -r line; do echo \"$line\" >> '{}'; done", stdin.display());
    let config = HostConfig { name: "worker".into(), generation: 1, state_dir: root.join("worker"), socket: None, idle_exit: None, provider: Some("claude".into()), runtime: vec!["sh".into(), "-c".into(), script] };
    (config, stdin)
}

fn session_record(root: &Path, parent: Option<&str>) {
    let session = root.join("worker");
    std::fs::create_dir_all(&session).unwrap();
    let record = json!({"name": "worker", "provider": "claude", "account": "c", "cwd": "/tmp", "model": null, "created_ms": 0, "host_pid": null, "conversation": "s", "host_dir": session, "parent": parent});
    std::fs::write(session.join("session.json"), record.to_string()).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_host_told_to_end_leaves_a_finished_worker_stopped_and_an_unfinished_one_parked() {
    for (reply, stopped) in [("Done: landed abc123.\nNext: none", true), ("Still waiting on the build.", false)] {
        let root = tempfile::tempdir().unwrap();
        session_record(root.path(), Some("lead"));
        let (config, _) = claude_turn(root.path(), 100_000, reply);
        let sink = Arc::new(Recorded::default());
        let watch = TurnWatch::new(Arc::new(Claude), sink.clone(), sink, Arc::new(Held::default()), root.path().to_path_buf(), root.path().join("worker"));
        let host = SessionHost::spawn_watched(&config, Watcher::new(Box::new(watch))).unwrap();
        host.end_with_host_signal();
        tokio::time::sleep(Duration::from_secs(1)).await;
        rustix::process::kill_process(rustix::process::getpid(), rustix::process::Signal::Term).unwrap();
        let code = tokio::time::timeout(Duration::from_secs(30), host.wait()).await.unwrap().unwrap();
        assert_eq!(code, 143);
        let record = crate::fleet::records::read_record(&root.path().join("worker")).unwrap();
        assert_eq!((record.stopped, record.ended), (stopped, stopped.then_some(Ending::HostEnded)), "{reply}");
    }
}

#[path = "silent_tests.rs"]
mod silent;
