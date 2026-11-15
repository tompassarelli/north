use std::path::Path;
use std::sync::{Arc, Mutex};

use north_fleet_supervisor::fleet::claims::Held;
use north_fleet_supervisor::fleet::fakes::Paper;
use north_fleet_supervisor::fleet::gates::GateRequest;
use north_fleet_supervisor::fleet::model::{Delivery, SessionRecord};
use north_fleet_supervisor::fleet::receipts::Courier;
use north_fleet_supervisor::fleet::records::write_record;
use north_fleet_supervisor::fleet::watch::TurnWatch;
use north_providers::providers::claude::fleet::Claude;
use serde_json::{Value, json};

#[derive(Default)]
struct Told(Mutex<Vec<String>>);

impl Courier for Told {
    fn notify(&self, parent: &str, text: String, _delivery: Delivery) {
        self.0.lock().unwrap().push(format!("{parent} {text}"));
    }

    fn ask(&self, _request: GateRequest) {}

    fn answer(&self, _host_dir: &Path, _frame: String) {}

    fn leads(&self, _parent: &str) -> bool {
        false
    }
}

#[test]
fn a_compacted_session_keeps_its_last_report() {
    let root = tempfile::tempdir().unwrap();
    let host = root.path().join("worker");
    std::fs::create_dir_all(&host).unwrap();
    let record: SessionRecord = serde_json::from_value(json!({"name": "worker", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": std::process::id(), "conversation": "s", "host_dir": host, "parent": "lead"})).unwrap();
    write_record(&host, &record).unwrap();
    let told = Arc::new(Told::default());
    let mut watch = TurnWatch::new(Arc::new(Claude), Arc::new(Paper::default()), told.clone(), Arc::new(Held::default()), root.path().to_path_buf(), host);
    let usage = json!({"input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0});
    let frames: [Value; 5] = [
        json!({"type": "user", "session_id": "s", "message": {"role": "user", "content": "Fix the parser."}}),
        json!({"type": "assistant", "session_id": "s", "parent_tool_use_id": null, "message": {"id": "m1", "role": "assistant", "content": [{"type": "text", "text": "Done: landed abc123.\nNext: none"}], "usage": usage}}),
        json!({"type": "result", "session_id": "s", "subtype": "success", "is_error": false, "num_turns": 1, "duration_ms": 10, "usage": usage}),
        json!({"type": "system", "subtype": "compact_boundary", "session_id": "s", "compact_metadata": {"post_tokens": 1000}}),
        json!({"type": "result", "session_id": "s", "subtype": "success", "is_error": false, "num_turns": 1, "duration_ms": 10, "usage": usage}),
    ];
    for frame in frames {
        watch.line(&serde_json::to_vec(&frame).unwrap());
    }
    watch.close();
    let told = told.0.lock().unwrap();
    assert!(!told.iter().any(|text| text.contains("ended with no")), "a session compacted after its report was flagged as ending its turn without one: {told:?}");
}
