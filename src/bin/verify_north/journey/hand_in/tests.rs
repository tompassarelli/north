use std::cell::Cell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::client::North;
use crate::exec::{Clock, Wall};
use crate::notes::Notes;

use super::super::{Journey, Plan};
use super::{hand_in, returned};
use crate::transcript::Transcript;

const WITHOUT_TOOLS: &str = r#"{"status":"idle","conversation":"c","entries":[{"kind":"user","text":"the brief"},{"kind":"tool","text":"Bash printf x > worker.txt"},{"kind":"assistant","text":"I have no hand-in tool"}]}"#;
const INIT: &str = r#"{"type":"system","subtype":"init","session_id":"c","mcp_servers":[{"name":"north","status":"failed"}]}"#;

struct Virtual {
    at: Cell<Instant>,
}

impl Clock for Virtual {
    fn now(&self) -> Instant {
        self.at.get()
    }

    fn pause(&self, span: Duration) {
        self.at.set(self.at.get() + span);
    }
}

#[test]
fn an_order_from_a_confined_seat_fails_the_spine_probe_naming_the_north_tool_server_its_worker_lost() {
    let root = tempfile::tempdir().unwrap();
    let program = root.path().join("fake-north");
    fs::write(&program, format!("#!/bin/sh\nprintf '%s\\n' '{WITHOUT_TOOLS}'\n")).unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
    let state = root.path().to_path_buf();
    let session = state.join("north/fleet/vn-0");
    let host = state.join("host/g1");
    fs::create_dir_all(&session).unwrap();
    fs::create_dir_all(&host).unwrap();
    fs::write(session.join("session.json"), format!(r#"{{"host_dir":{:?}}}"#, host.parent().unwrap())).unwrap();
    fs::write(host.join("stream.jsonl"), format!("{INIT}\n")).unwrap();
    let north = North::new(program, state.clone(), state.clone(), None);
    let clock = Rc::new(Virtual { at: Cell::new(Wall.now()) });
    let mut journey = Journey { north, notes: Notes::new(None), plan: Plan { stamp: 0, claude_account: None, state }, session: "vn-0".to_owned(), scratch: None, lander: None, broker: None, landed: None, clock };
    let failure = hand_in(&mut journey).unwrap_err();
    assert!(failure.contains("did not call the mcp__north__hand-in tool, since its tool server north did not start"), "{failure}");

    let batch = r#"{"status":"idle","conversation":"c","entries":[{"kind":"user","text":"the brief"},{"kind":"tool","text":"Bash printf 'the worker's change\\n' > worker.txt && cat worker.txt"},{"kind":"tool","text":"mcp__north__hand-in worker.txt"},{"kind":"result","text":"the worker's change"},{"kind":"result","text":"Done: queued 1-hand-in.json"},{"kind":"assistant","text":"Done: queued 1-hand-in.json"}]}"#;
    let transcript = Transcript::parse(&serde_json::from_str(batch).unwrap());
    assert_eq!(returned(&transcript, 2), Ok(()));
}
