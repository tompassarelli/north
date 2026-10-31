use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::fleet::runtimes::Runtimes;

use super::Fleet;

pub(super) struct Session {
    pub(super) frames: Arc<Mutex<Vec<String>>>,
}

pub(super) fn fleet(home: &Path) -> Fleet {
    let ledger = Arc::new(crate::ledger::Ledger::open(&home.join("ledger.db")).unwrap());
    Fleet::at(home.into(), Runtimes::new(vec![Arc::new(crate::providers::claude::fleet::Claude)]), ledger).unwrap().arming(|_| Ok(()))
}

pub(super) fn live(fleet: &Fleet, name: &str) -> Session {
    live_under(fleet, name, None)
}

pub(super) fn live_under(fleet: &Fleet, name: &str, parent: Option<&str>) -> Session {
    let dir = fleet.root.join(name);
    std::fs::create_dir_all(dir.join("g1")).unwrap();
    let record = serde_json::json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": std::process::id(), "conversation": name, "parent": parent, "host_dir": dir, "stream_offset": 0});
    std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
    let assistant = serde_json::json!({"type": "assistant", "session_id": name, "message": {"content": [{"type": "text", "text": "Not done: waiting."}]}});
    let result = serde_json::json!({"type": "result", "subtype": "success", "session_id": name});
    std::fs::write(dir.join("g1/stream.jsonl"), format!("{assistant}\n{result}\n")).unwrap();
    let listener = std::os::unix::net::UnixListener::bind(dir.join("host.sock")).unwrap();
    let frames = Arc::new(Mutex::new(Vec::new()));
    let seen = frames.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut writer = &stream;
            for line in BufReader::new(&stream).lines().map_while(Result::ok) {
                seen.lock().unwrap().push(line);
                writeln!(writer, "{{\"host_seq\":1}}").unwrap();
            }
        }
    });
    Session { frames }
}
