use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::json;

use crate::fleet::fakes::{Paper, Stream};
use crate::fleet::registry::{Fleet, Restart};
use crate::fleet::runtimes::Runtimes;

fn fleet(home: &Path) -> Fleet {
    Fleet::at(home.into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default())).unwrap().arming(|_| Ok(()))
}

fn stopped(fleet: &Fleet, name: &str, model: &str) -> PathBuf {
    let dir = fleet.root.join(name);
    std::fs::create_dir_all(dir.join("g1")).unwrap();
    let record = json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": model, "created_ms": 1, "host_pid": null, "conversation": "old-conversation", "host_dir": dir, "stream_offset": 0});
    std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
    let done = json!({"type": "assistant", "session_id": "old-conversation", "message": {"content": [{"type": "text", "text": "Not done: the build host refused the lease."}]}});
    std::fs::write(dir.join("g1/stream.jsonl"), format!("{done}\n")).unwrap();
    dir
}

#[test]
fn a_relaunch_across_model_families_starts_a_fresh_conversation_that_follows_the_last_report() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path());
    stopped(&fleet, "worker", "opus");
    let plans = Arc::new(Mutex::new(Vec::new()));
    let restarted = fleet
        .relaunch_with("worker", Some("sonnet"), None, |_, restart| {
            plans.lock().unwrap().push(restart);
            fleet.record("worker")
        })
        .unwrap();

    let Some(Restart::Fresh { conversation, follows }) = plans.lock().unwrap().pop() else {
        panic!("a relaunch onto another family resumed the conversation");
    };
    assert!(conversation.is_some() && conversation != Some("old-conversation".to_owned()), "{conversation:?}");
    assert_eq!(restarted.conversation.as_deref(), conversation.as_deref(), "the record keeps the fresh conversation");    assert!(follows.starts_with("Follows: worker (relaunched from opus to sonnet"), "{follows}");
    assert!(follows.ends_with("Not done: the build host refused the lease."), "{follows}");

    let same = fleet
        .relaunch_with("worker", Some("claude-sonnet-5-5"), None, |_, restart| {
            plans.lock().unwrap().push(restart);
            fleet.record("worker")
        })
        .unwrap();
    assert_eq!(same.conversation, conversation, "a relaunch within one family resumes the conversation");
    assert!(matches!(plans.lock().unwrap().pop(), Some(Restart::Resume)), "a relaunch within one family resumed");
}

#[test]
fn a_restart_on_another_model_family_keeps_replies_in_the_transcript() {
    use std::io::Write;
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path());
    stopped(&fleet, "worker", "opus");
    let mut follower = fleet.follow("worker").unwrap();
    let restarted = fleet.relaunch_with("worker", Some("sonnet"), None, |_, _| fleet.record("worker")).unwrap();
    let conversation = restarted.conversation.unwrap();
    assert_ne!(conversation, "worker");
    let reply = json!({"type": "assistant", "session_id": &conversation, "message": {"content": [{"type": "text", "text": "Done: alive."}]}});
    let mut stream = std::fs::OpenOptions::new().append(true).open(fleet.record("worker").unwrap().host_dir.join("g1/stream.jsonl")).unwrap();
    writeln!(stream, "{reply}").unwrap();
    follower.poll().unwrap();
    assert!(follower.transcript().entries.iter().any(|entry| entry.text == "Done: alive."), "{:?}", follower.transcript().entries);
    assert_eq!(fleet.record("worker").unwrap().conversation, Some(conversation), "the record keeps the conversation the client writes");
}
