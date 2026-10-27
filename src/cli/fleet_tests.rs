use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::*;
use crate::fleet::books::{Books, Landing, SessionSpend};
use crate::fleet::model::Delivery;
use crate::fleet::gates::{ASKED_REPLY, GateRequest};
use crate::fleet::registry::TurnWatch;
use crate::fleet::receipts::{Courier, ReceiptBook};
use crate::fleet::turns::{RunRecord, RunSink, TurnExit};
use crate::session_host::{SessionHost, Watcher};

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
    fn post(&self, parent: &str, text: String, delivery: Delivery) {
        self.1.lock().unwrap().push((parent.to_owned(), text, delivery));
    }

    fn ask(&self, request: GateRequest) {
        self.2.lock().unwrap().push(request);
    }

    fn answer(&self, host_dir: &Path, frame: String) {
        self.3.lock().unwrap().push((host_dir.to_path_buf(), frame));
    }
}

impl Books for Recorded {
    fn spend(&self) -> NorthResult<Vec<SessionSpend>> {
        Ok(Vec::new())
    }

    fn landed_since(&self, _since_ms: u64) -> NorthResult<Vec<Landing>> {
        Ok(Vec::new())
    }

    fn record_landing(&self, _task_ref: &str, _sha: &str, _landed_ms: u64, _wall_ms: Option<u64>, _review: Option<&str>) -> NorthResult<bool> {
        Ok(false)
    }
}

fn silent_codex_turn(root: &Path, silent_secs: u64) -> HostConfig {
    let thread = r#""threadId":"th-1""#;
    let script = format!(
        r#"echo '{{"method":"turn/started","params":{{{thread},"turn":{{"id":"turn-1"}}}}}}'; sleep {silent_secs}; echo '{{"method":"turn/completed","params":{{{thread},"turn":{{"id":"turn-1","status":"completed","durationMs":{}}}}}}}'; exec sleep 60"#,
        silent_secs * 1000
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
async fn a_silent_codex_turn_is_never_parked_mid_turn() {
    let root = tempfile::tempdir().unwrap();
    let config = silent_codex_turn(root.path(), 3);
    let sink = Arc::new(Recorded::default());
    let watch = TurnWatch::new(Arc::new(Codex), sink.clone(), sink.clone(), root.path().to_path_buf(), root.path().to_path_buf());
    let mut host = SessionHost::spawn_watched(&config, Watcher::new(Box::new(watch))).unwrap();
    let started = Instant::now();
    host.rest_when_idle(Some(Duration::from_secs(1)));
    let code = tokio::time::timeout(Duration::from_secs(20), host.wait()).await.unwrap().unwrap();
    assert_eq!(code, 143, "the host parks by SIGTERM once the turn has ended");
    assert!(started.elapsed() > Duration::from_secs(3), "parked after {:?}, inside the 3 s silent turn", started.elapsed());
    let runs = sink.0.lock().unwrap();
    assert_eq!(runs.iter().map(|run| (run.turn.exit, run.turn.wall_ms)).collect::<Vec<_>>(), [(TurnExit::Completed, 3000)]);
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
    let watch = TurnWatch::new(Arc::new(Codex), Arc::new(Recorded::default()), Arc::new(Recorded::default()), root.path().to_path_buf(), root.path().to_path_buf());
    let mut host = SessionHost::spawn_watched(&config, Watcher::new(Box::new(watch))).unwrap();
    host.rest_when_idle(Some(Duration::from_secs(1)));
    let parked = tokio::time::timeout(Duration::from_secs(10), host.wait()).await;
    assert_eq!(parked.ok().map(Result::unwrap), Some(143), "an idle host must park despite steady non-turn output");
}

#[test]
fn text_show_marks_urgent_user_messages_and_indents_their_lines() {
    let urgent = crate::fleet::registry::urgent_prefixed("stop\nand look");
    let lines: Vec<String> = [Entry { kind: EntryKind::User, text: urgent }, Entry { kind: EntryKind::User, text: "plain".into() }].iter().map(entry_line).collect();
    assert_eq!(lines, ["user (urgent): stop\n  and look", "user: plain"]);
}

#[test]
fn a_deepened_turn_is_tagged_with_its_session_prompt_sha() {
    let root = tempfile::tempdir().unwrap();
    let session = root.path().join("deep");
    std::fs::create_dir_all(&session).unwrap();
    let record = json!({
        "name": "deep", "provider": "claude", "account": "c", "cwd": "/tmp", "model": null, "created_ms": 0,
        "host_pid": null, "conversation": "s", "host_dir": session, "system_prompt_sha": "abc"
    });
    std::fs::write(session.join("session.json"), record.to_string()).unwrap();
    let sink = Arc::new(Recorded::default());
    let mut watch = TurnWatch::new(Arc::new(Claude), sink.clone(), sink.clone(), root.path().to_path_buf(), session.clone());
    let prompt = crate::fleet::prompt::deepened("fix the parser");
    for frame in [
        json!({"type": "user", "isReplay": true, "session_id": "s", "uuid": "u", "message": {"content": prompt}}),
        json!({"type": "result", "subtype": "success", "session_id": "s", "is_error": false}),
    ] {
        watch.line(frame.to_string().as_bytes());
    }
    let runs = sink.0.lock().unwrap();
    assert_eq!(runs.iter().map(|run| (run.session.as_str(), run.deepen, run.system_prompt_sha.as_deref())).collect::<Vec<_>>(), [("deep", true, Some("abc"))]);
}

#[test]
fn every_model_fleet_can_launch_has_a_price() {
    let prices = crate::ledger::pricing::PriceTable::load().unwrap();
    let launchable: Vec<(String, &str)> = runtimes().providers().into_iter().flat_map(|provider| {
        let runtime = runtimes().get(&provider).unwrap();
        runtime.models().iter().map(move |model| (provider.label().to_owned(), *model)).collect::<Vec<_>>()
    }).collect();
    assert!(launchable.iter().any(|(provider, _)| provider == "codex") && launchable.iter().any(|(_, model)| *model == "haiku"));
    let unpriced: Vec<_> = launchable.iter().filter(|(_, model)| prices.price(model).is_none()).collect();
    assert!(unpriced.is_empty(), "launchable models without a price: {unpriced:?}");
}

#[test]
fn a_child_delivers_each_finished_turn_in_order_while_its_parent_is_busy() {
    let root = tempfile::tempdir().unwrap();
    let session = root.path().join("kid");
    std::fs::create_dir_all(&session).unwrap();
    let record = json!({
        "name": "kid", "provider": "claude", "account": "c", "cwd": "/tmp", "model": null, "created_ms": 0,
        "host_pid": null, "conversation": "s", "host_dir": session, "parent": "boss"
    });
    std::fs::write(session.join("session.json"), record.to_string()).unwrap();
    let sink = Arc::new(Recorded::default());
    let mut watch = TurnWatch::new(Arc::new(Claude), sink.clone(), sink.clone(), root.path().to_path_buf(), session.clone());
    let user = |turn: u8| json!({"type": "user", "isReplay": true, "session_id": "s", "uuid": format!("u{turn}"), "message": {"content": format!("run step {turn}")}});
    let said = |text: &str| json!({"type": "assistant", "session_id": "s", "parent_tool_use_id": null, "message": {"content": [{"type": "text", "text": text}]}});
    let task = json!({"type": "system", "subtype": "task_started", "session_id": "s", "task_id": "t1"});
    let result = json!({"type": "result", "subtype": "success", "session_id": "s", "is_error": false});
    watch.line(task.to_string().as_bytes());
    for (turn, report) in [(1, "Done: first"), (2, "Blocked: second"), (3, "Split: third")] {
        for frame in [user(turn), said(report), result.clone()] {
            watch.line(frame.to_string().as_bytes());
        }
    }
    watch.line(json!({"type": "system", "subtype": "task_notification", "session_id": "s", "task_id": "t1"}).to_string().as_bytes());
    let posted = sink.1.lock().unwrap();
    assert_eq!(*posted, [
        ("boss".to_owned(), "[from kid · turn 1 · exit completed] Done: first".to_owned(), Delivery::Queued),
        ("boss".to_owned(), "[from kid · turn 2 · exit completed] Blocked: second".to_owned(), Delivery::Queued),
        ("boss".to_owned(), "[from kid · turn 3 · exit completed] Split: third".to_owned(), Delivery::Queued),
    ]);
}

#[test]
fn progress_questions_and_empty_turns_stay_in_the_ledger_while_only_final_reports_reach_the_parent() {
    let root = tempfile::tempdir().unwrap();
    let session = root.path().join("kid");
    std::fs::create_dir_all(&session).unwrap();
    let record = json!({
        "name": "kid", "provider": "claude", "account": "c", "cwd": "/tmp", "model": null, "created_ms": 0,
        "host_pid": null, "conversation": "s", "host_dir": session, "parent": "boss"
    });
    std::fs::write(session.join("session.json"), record.to_string()).unwrap();
    let sink = Arc::new(Recorded::default());
    let mut watch = TurnWatch::new(Arc::new(Claude), sink.clone(), sink.clone(), root.path().to_path_buf(), session);
    for frame in include_str!("../fixtures/claude-stream/wake-receipts.jsonl").lines() {
        watch.line(frame.as_bytes());
    }
    assert_eq!(sink.0.lock().unwrap().len(), 8);
    let posted = sink.1.lock().unwrap();
    assert_eq!(*posted, [
        ("boss".into(), "[from kid · turn 7 · exit completed] Done: all monitors finished".into(), Delivery::Queued),
    ]);
}

#[test]
fn a_codex_question_opens_a_gate_for_its_session_and_unblocks_the_turn() {
    let root = tempfile::tempdir().unwrap();
    let host = root.path().join(".hosts/codex-pooled");
    let session = root.path().join("kid");
    std::fs::create_dir_all(&session).unwrap();
    let record = json!({
        "name": "kid", "provider": "codex", "account": "pooled", "cwd": "/tmp", "model": null, "created_ms": 0,
        "host_pid": null, "conversation": "th-1", "host_dir": host, "parent": "boss"
    });
    std::fs::write(session.join("session.json"), record.to_string()).unwrap();
    let sink = Arc::new(Recorded::default());
    let mut watch = TurnWatch::new(Arc::new(Codex), sink.clone(), sink.clone(), root.path().to_path_buf(), host.clone());
    let asked = |thread: &str, id: u64| json!({"id": id, "method": "item/tool/requestUserInput", "params": {"threadId": thread, "turnId": "t", "itemId": "q",
        "questions": [{"id": "db", "header": "Database", "question": "Which database?", "options": [{"label": "sqlite (Recommended)", "description": ""}, {"label": "postgres", "description": ""}]}]}});
    watch.line(asked("th-1", 4).to_string().as_bytes());
    watch.line(asked("stranger", 5).to_string().as_bytes());
    let gates = sink.2.lock().unwrap();
    assert_eq!(gates.iter().map(|gate| (gate.session.as_str(), gate.question.as_str(), gate.default)).collect::<Vec<_>>(), [("kid", "Database: Which database?", 1)]);
    let answers = sink.3.lock().unwrap();
    assert_eq!(answers.len(), 1, "only a fleet session's question is answered");
    let reply: Value = serde_json::from_str(&answers[0].1).unwrap();
    assert_eq!((answers[0].0.as_path(), reply["id"].as_u64(), reply["result"]["answers"]["db"]["answers"][0].as_str()), (host.as_path(), Some(4), Some(ASKED_REPLY)));
}

#[test]
fn a_launch_inherits_the_calling_session_as_parent_unless_told_none() {
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
    assert_eq!(parent(&[], None), None);
}

fn seconds(rollover: u64, finished_exit: u64) -> crate::fleet::idle::Policy {
    crate::fleet::idle::Policy { rollover_after: Duration::from_secs(rollover), cache_lifetime: Duration::from_secs(60), rollover_min_tokens: 200_000, finished_exit_after: Duration::from_secs(finished_exit) }
}

fn claude_turn(root: &Path, context: u64, reply: &str) -> (HostConfig, PathBuf) {
    let usage = json!({"input_tokens": 1, "output_tokens": 5, "cache_read_input_tokens": context - 1, "cache_creation_input_tokens": 0});
    let assistant = json!({"type": "assistant", "session_id": "s", "parent_tool_use_id": null, "message": {"id": "m1", "content": [{"type": "text", "text": reply}], "usage": usage}});
    let result = json!({"type": "result", "subtype": "success", "session_id": "s", "is_error": false});
    let stdin = root.join("stdin.jsonl");
    let script = format!("echo '{assistant}'; echo '{result}'; while read -r line; do echo \"$line\" >> '{}'; done", stdin.display());
    let config = HostConfig { name: "worker".into(), generation: 1, state_dir: root.join("worker"), socket: None, idle_exit: None, provider: Some("claude".into()), runtime: vec!["sh".into(), "-c".into(), script] };
    (config, stdin)
}

fn session_record(root: &Path, parent: Option<&str>) {
    let session = root.join("worker");
    std::fs::create_dir_all(&session).unwrap();
    let record = json!({"name": "worker", "provider": "claude", "account": "c", "cwd": "/tmp", "model": null, "created_ms": 0, "host_pid": null, "conversation": "s", "host_dir": session, "parent": parent});
    std::fs::write(session.join("session.json"), record.to_string()).unwrap();
}

async fn rest(root: &Path, context: u64, reply: &str, policy: crate::fleet::idle::Policy, park_after: Option<Duration>) -> (u8, String) {
    let (config, stdin) = claude_turn(root, context, reply);
    let sink = Arc::new(Recorded::default());
    let watch = TurnWatch::new(Arc::new(Claude), sink.clone(), sink, root.to_path_buf(), root.join("worker")).idling_by(policy);
    let mut host = SessionHost::spawn_watched(&config, Watcher::new(Box::new(watch))).unwrap();
    host.rest_when_idle(park_after);
    let code = tokio::time::timeout(Duration::from_secs(30), host.wait()).await.unwrap().unwrap();
    (code, std::fs::read_to_string(stdin).unwrap_or_default())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_idle_session_over_200k_is_sent_compact_at_its_rollover_then_parks() {
    let root = tempfile::tempdir().unwrap();
    session_record(root.path(), Some("lead"));
    let (code, stdin) = rest(root.path(), 250_000, "Still waiting on the build.", seconds(1, 60), Some(Duration::from_secs(3))).await;
    assert_eq!(code, 143);
    assert_eq!(stdin.lines().filter(|line| line.contains(r#""content":"/compact""#)).count(), 1, "one rollover before parking: {stdin}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_idle_session_under_200k_parks_untouched() {
    let root = tempfile::tempdir().unwrap();
    session_record(root.path(), Some("lead"));
    let (code, stdin) = rest(root.path(), 150_000, "Still waiting on the build.", seconds(5, 60), Some(Duration::from_secs(1))).await;
    assert_eq!((code, stdin.as_str()), (143, ""));
    assert!(!crate::fleet::records::read_record(&root.path().join("worker")).unwrap().stopped, "parked, not exited");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finished_worker_exits_at_its_idle_limit_uncompacted_and_marked_exited() {
    let root = tempfile::tempdir().unwrap();
    session_record(root.path(), Some("lead"));
    let (code, stdin) = rest(root.path(), 400_000, "Done: landed abc123.", seconds(1, 60), Some(Duration::from_secs(2))).await;
    assert_eq!((code, stdin.as_str()), (143, ""));
    assert!(crate::fleet::records::read_record(&root.path().join("worker")).unwrap().stopped);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_finished_root_session_is_not_exited() {
    let root = tempfile::tempdir().unwrap();
    session_record(root.path(), None);
    let (code, _) = rest(root.path(), 100_000, "Done: ticked.", seconds(1, 1), Some(Duration::from_secs(2))).await;
    assert_eq!(code, 143);
    assert!(!crate::fleet::records::read_record(&root.path().join("worker")).unwrap().stopped, "a root session parks, never exits");
}

#[test]
fn a_worker_on_the_handoff_arm_is_told_to_hand_off_at_its_threshold_and_again_per_further_50k_even_after_a_revive() {
    let calls = |handoff_at: Option<u64>| {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("worker");
        std::fs::create_dir_all(&session).unwrap();
        let record = json!({"name": "worker", "provider": "claude", "account": "c", "cwd": "/tmp", "model": null, "created_ms": 0, "host_pid": null, "conversation": "s", "host_dir": session, "parent": "lead", "handoff_at": handoff_at});
        std::fs::write(session.join("session.json"), record.to_string()).unwrap();
        let sink = Arc::new(Recorded::default());
        let mut watch = TurnWatch::new(Arc::new(Claude), sink.clone(), sink.clone(), root.path().to_path_buf(), session);
        for (id, context) in [("m1", 349_000), ("m2", 360_000), ("m3", 400_000), ("m4", 410_000)] {
            let usage = json!({"input_tokens": 1, "output_tokens": 1, "cache_read_input_tokens": context - 1, "cache_creation_input_tokens": 0});
            watch.line(json!({"type": "assistant", "session_id": "s", "parent_tool_use_id": null, "message": {"id": id, "content": [], "usage": usage}}).to_string().as_bytes());
        }
        sink.3.lock().unwrap().iter().map(|(_, frame)| frame.clone()).collect::<Vec<_>>()
    };
    let nudged = calls(Some(350_000));
    assert_eq!(nudged.len(), 2, "at 360k and at 410k, not at 400k: {nudged:?}");
    assert!(nudged[0].contains("Your context is 360k tokens, past the 350k handoff point") && nudged[0].contains("worker.md"), "{}", nudged[0]);
    assert!(calls(None).is_empty(), "the no-handoff arm and unarmed sessions are never nudged");
}
