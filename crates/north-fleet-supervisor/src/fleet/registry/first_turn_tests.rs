use std::fs::OpenOptions;
use std::io::Write;
use std::time::Duration;

use serde_json::json;

use super::super::fixtures::{fleet, live, live_under};
use super::FirstTurn;
use crate::fleet::transcript::stream_path;

#[test]
fn a_worker_whose_tool_server_failed_to_start_is_reported_to_its_parent() {
    let home = tempfile::tempdir().unwrap();
    let fleet = fleet(home.path());
    let lead = live(&fleet, "lead");
    let worker = live_under(&fleet, "worker", Some("lead"));
    let record = fleet.record("worker").unwrap();
    let stream = stream_path(&record.host_dir);
    let prompted = worker.frames;
    let client = std::thread::spawn(move || {
        assert!(north_base::platform::wait::until(Duration::from_secs(30), || !prompted.lock().unwrap().is_empty()), "the first turn sent no prompt");
        let mut file = OpenOptions::new().append(true).open(&stream).unwrap();
        let init = json!({"type": "system", "subtype": "init", "session_id": "worker", "mcp_servers": [{"name": "north", "status": "failed"}]});
        let prompt = json!({"type": "user", "session_id": "worker", "message": {"role": "user", "content": "start the box"}});
        let answer = json!({"type": "assistant", "session_id": "worker", "message": {"content": [{"type": "text", "text": "working"}]}});
        for frame in [init, prompt, answer] {
            writeln!(file, "{frame}").unwrap();
        }
    });

    assert_eq!(fleet.first_turn(&record, "start the box").unwrap(), FirstTurn::Answered);
    client.join().unwrap();
    let told = lead.frames.lock().unwrap();
    assert!(told.iter().any(|frame| frame.contains("[from worker] its tool server north did not start")), "the lead was not told its worker runs without north's tools: {told:?}");
}
