use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use super::*;
use crate::fleet::reaper::{self, Evidence, Unlanded, Work};
use crate::fleet::model::SessionRecord;
use crate::fleet::run_recording::{RetiredRun, RunRecording};
use crate::fleet::runtimes::Runtimes;

struct Recorded(Mutex<Vec<RetiredRun>>);

impl RunRecording for Recorded {
    fn ended(&self, _conversation: &str) {}

    fn retired(&self, run: &RetiredRun) {
        self.0.lock().unwrap().push(run.clone());
    }
}

struct Nothing;

impl Evidence for Nothing {
    fn landed(&self, _cwd: &Path, _sha: &str) -> bool {
        false
    }

    fn running(&self, _records: &[&SessionRecord]) -> BTreeMap<String, Work> {
        BTreeMap::new()
    }

    fn lanes(&self) -> BTreeMap<String, Vec<north_base::land_ticket::Lane>> {
        BTreeMap::new()
    }

    fn lane(&self, _record: &SessionRecord) -> Option<Unlanded> {
        None
    }
}

#[test]
fn a_session_record_that_is_exited_and_quiet_past_the_stale_window_is_retired_and_its_report_and_stop_reason_stay_readable_and_one_runs_row_carries_the_stop_reason() {
    let home = tempfile::tempdir().unwrap();
    let ledger = Arc::new(crate::ledger::Ledger::open(&home.path().join("ledger.db")).unwrap());
    let rows = Arc::new(Recorded(Mutex::default()));
    let fleet = Fleet::at(home.path().into(), Runtimes::new(vec![Arc::new(crate::providers::claude::fleet::Claude)]), ledger).unwrap().recording_runs(rows.clone());
    let dir = home.path().join(".local/state/north/fleet/exited");
    std::fs::create_dir_all(dir.join("g1")).unwrap();
    let record = json!({"name": "exited", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": "exited", "host_dir": dir, "stream_offset": 0, "stopped": true, "task_ref": "tompassarelli/north#1235", "box_name": "a box"});
    std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
    let report = format!("Not done: {}\nStop: decision", "the box needs a choice. ".repeat(250));
    assert!(report.len() > 4000);
    let said = json!({"type": "assistant", "session_id": "exited", "message": {"content": [{"type": "text", "text": report}]}});
    let result = json!({"type": "result", "subtype": "success", "session_id": "exited"});
    std::fs::write(dir.join("g1/stream.jsonl"), format!("{said}\n{result}\n")).unwrap();
    assert_eq!(fleet.status(&fleet.session("exited").unwrap()), Status::Exited);

    let inside = reaper::reap_after(&fleet, &Nothing, Duration::from_secs(24 * 3600), crate::fleet::runtime::now_ms(), true);
    assert!(inside.retired.is_empty() && fleet.session("exited").is_ok(), "an exited record inside the stale window stays: {inside:?}");
    let past = reaper::reap_after(&fleet, &Nothing, Duration::ZERO, crate::fleet::runtime::now_ms(), true);
    assert_eq!(past.retired.iter().map(|retired| retired.name.as_str()).collect::<Vec<_>>(), ["exited"], "{past:?}");
    assert!(fleet.session("exited").is_err());
    let again = reaper::reap_after(&fleet, &Nothing, Duration::ZERO, crate::fleet::runtime::now_ms(), true);
    assert!(again.retired.is_empty(), "{again:?}");
    let written = rows.0.lock().unwrap().clone();
    assert_eq!(written.len(), 1, "one runs row per retired session: {written:?}");
    let row = super::super::run_ingest::retired_row(&written[0]);
    assert_eq!((row["outcome"].as_str(), row["stop"].as_str(), row["item"].as_str(), row["box"].as_str(), row["fleet_name"].as_str()), (Some("not done"), Some("Stop: decision"), Some("tompassarelli/north#1235"), Some("a box"), Some("exited")), "{row}");
    let kept = fleet.follow_retired("exited").unwrap().transcript().entries.iter().any(|entry| entry.text.starts_with("Not done:"));
    assert!(kept, "the retired session's report stays readable");

    let mut printed = Vec::new();
    show_to(&fleet, "exited", false, &mut printed).unwrap();
    let printed = String::from_utf8(printed).unwrap();

    assert!(printed.contains(report.replace('\n', "\n  ").trim_end()), "the whole report is printed: {printed}");
    assert!(printed.trim_end().ends_with(']'), "the closing status line follows: {printed}");
}
