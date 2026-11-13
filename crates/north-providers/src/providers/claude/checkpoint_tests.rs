use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use crate::fleet::claims::Held;
use crate::fleet::fakes::Paper;
use crate::fleet::gates::GateRequest;
use crate::fleet::model::Delivery;
use crate::fleet::receipts::Courier;
use crate::fleet::session_host::Rest;
use crate::fleet::registry::TurnWatch;
use super::Claude;

#[derive(Default)]
struct Heard(Mutex<Vec<String>>);

impl Courier for Heard {
    fn notify(&self, _parent: &str, _text: String, _delivery: Delivery) {}

    fn ask(&self, _request: GateRequest) {}

    fn answer(&self, _host_dir: &Path, frame: String) {
        self.0.lock().unwrap().push(frame);
    }
}

struct Worker {
    _root: tempfile::TempDir,
    watch: TurnWatch,
    heard: Arc<Heard>,
}

fn worker() -> Worker {
    let root = tempfile::tempdir().unwrap();
    let host = root.path().join("worker");
    std::fs::create_dir_all(&host).unwrap();
    let record = json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": "claude-opus-5-5", "created_ms": 1, "host_pid": null, "conversation": "s", "host_dir": host, "parent": "lead"});
    std::fs::write(host.join("session.json"), record.to_string()).unwrap();
    let heard = Arc::new(Heard::default());
    let watch = TurnWatch::new(Arc::new(Claude), Arc::new(Paper::default()), heard.clone(), Arc::new(Held::default()), root.path().to_path_buf(), host);
    Worker { _root: root, watch, heard }
}

impl Worker {
    fn feed(&mut self, frame: Value) {
        self.watch.line(frame.to_string().as_bytes());
    }

    fn call(&mut self, context: u64, text: &str) {
        self.feed(json!({"type": "assistant", "session_id": "s", "parent_tool_use_id": null, "message": {"id": format!("m{context}{}", text.len()), "content": [{"type": "text", "text": text}], "usage": {"input_tokens": 1, "cache_read_input_tokens": context, "cache_creation_input_tokens": 0, "output_tokens": 9}}}));
    }

    fn turn_ends(&mut self) {
        self.feed(json!({"type": "result", "subtype": "success", "session_id": "s", "usage": {}}));
    }

    fn questions(&self) -> usize {
        self.heard.0.lock().unwrap().iter().filter(|frame| frame.contains("Turns left: N. Focus: TEXT")).count()
    }

    fn sent_when_idle(&mut self) -> Vec<String> {
        match self.watch.rest(Duration::from_secs(1), None) {
            Rest::Send(frames) => frames,
            Rest::Wait | Rest::End => Vec::new(),
        }
    }
}

#[test]
fn a_worker_answering_forty_turns_left_at_260k_is_sent_its_focus_as_a_compact_and_resumed_once_after_it_and_one_answering_three_is_sent_nothing() {
    let mut long = worker();
    long.call(249_000, "reading the code");
    assert_eq!(long.questions(), 0);
    long.call(260_000, "reading the code");
    assert_eq!(long.questions(), 1);
    long.call(260_500, "Turns left: 40. Focus: keep the lane path and the box list");
    assert!(long.sent_when_idle().is_empty(), "the turn has not ended");
    long.turn_ends();
    let frames = long.sent_when_idle();
    assert_eq!(frames.len(), 1, "{frames:?}");
    let frame: Value = serde_json::from_str(&frames[0]).unwrap();
    assert_eq!(frame["message"]["content"], "/compact keep the lane path and the box list");
    assert!(long.sent_when_idle().is_empty());

    long.feed(json!({"type": "system", "subtype": "compact_boundary", "session_id": "s", "compact_metadata": {"trigger": "manual", "pre_tokens": 260_500, "post_tokens": 9_000}}));
    long.turn_ends();
    let frames = long.sent_when_idle();
    assert_eq!(frames.len(), 1, "one resume turn after the compaction: {frames:?}");
    assert!(frames[0].contains("Continue it from your focus"), "{frames:?}");
    assert!(long.sent_when_idle().is_empty(), "exactly one resume turn");
    long.call(250_000, "working again");
    assert_eq!(long.questions(), 2, "a compaction re-arms the checkpoint at 250k");

    let mut near = worker();
    near.call(260_000, "reading the code");
    near.call(260_500, "Turns left: 3. Focus: finish the last box");
    near.turn_ends();
    assert_eq!(near.questions(), 1);
    assert!(near.sent_when_idle().is_empty());
    near.feed(json!({"type": "system", "subtype": "compact_boundary", "session_id": "s", "compact_metadata": {"trigger": "auto", "pre_tokens": 400_000, "post_tokens": 9_000}}));
    near.turn_ends();
    assert!(near.sent_when_idle().is_empty(), "a compaction the checkpoint did not send owes no resume turn");
}

