use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};

use super::Fleet;

pub struct Session {
    pub frames: Arc<Mutex<Vec<String>>>,
}

#[cfg(test)]
pub(crate) fn fleet(home: &std::path::Path) -> Fleet {
    use crate::fleet::fakes::{Paper, Stream};
    use crate::fleet::runtimes::Runtimes;
    Fleet::at(home.into(), Runtimes::new(vec![Arc::new(Stream)]), Arc::new(Paper::default())).unwrap().arming(|_| Ok(()))
}

pub fn live(fleet: &Fleet, name: &str) -> Session {
    live_under(fleet, name, None)
}

/// A live lead holding `task`, running in the main checkout of a project under the `code` directory of the fleet's home.
#[cfg(test)]
pub(crate) fn lead(fleet: &Fleet, name: &str, task: &str) -> Session {
    let session = live(fleet, name);
    let main = fleet.home.join("code/north/main");
    std::fs::create_dir_all(&main).unwrap();
    let path = fleet.root.join(name).join("session.json");
    let mut record: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    record["role"] = "lead".into();
    record["task_ref"] = task.into();
    record["cwd"] = main.display().to_string().into();
    std::fs::write(&path, record.to_string()).unwrap();
    session
}

pub(crate) fn live_under(fleet: &Fleet, name: &str, parent: Option<&str>) -> Session {
    let dir = fleet.root.join(name);
    std::fs::create_dir_all(dir.join("g1")).unwrap();
    let record = serde_json::json!({"name": name, "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": std::process::id(), "conversation": name, "parent": parent, "host_dir": dir, "stream_offset": 0});
    std::fs::write(dir.join("session.json"), record.to_string()).unwrap();
    let assistant = serde_json::json!({"type": "assistant", "session_id": name, "message": {"content": [{"type": "text", "text": "Not done: waiting."}]}});
    let result = serde_json::json!({"type": "result", "subtype": "success", "session_id": name});
    std::fs::write(dir.join("g1/stream.jsonl"), format!("{assistant}\n{result}\n")).unwrap();
    Session { frames: answering(&dir, false) }
}

/// Stands in for the session host a start brings up in `dir`: this process is its pid, its socket answers every frame, and its client answers each one with a turn of the conversation the record names.
#[cfg(test)]
pub(crate) fn standing_host(dir: &std::path::Path, _: &crate::fleet::runtime::HostSpec, _: &crate::fleet::model::ProviderName, _: Option<std::time::Duration>, _: &str) -> crate::error::NorthResult<u32> {
    std::fs::create_dir_all(dir.join("g1"))?;
    std::fs::write(north_base::platform::session_host_files::pid_path(dir), std::process::id().to_string())?;
    answering(dir, true);
    Ok(std::process::id())
}

fn answering(dir: &std::path::Path, turns: bool) -> Arc<Mutex<Vec<String>>> {
    let session = dir.to_path_buf();
    let socket = north_base::platform::session_host_files::socket_path(dir);
    let _ = std::fs::remove_file(&socket);
    let listener = std::os::unix::net::UnixListener::bind(socket).unwrap();
    let frames = Arc::new(Mutex::new(Vec::new()));
    let seen = frames.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut writer = &stream;
            for line in BufReader::new(&stream).lines().map_while(Result::ok) {
                seen.lock().unwrap().push(line);
                if turns {
                    take_turn(&session);
                }
                writeln!(writer, "{{\"host_seq\":1}}").unwrap();
            }
        }
    });
    frames
}

fn take_turn(dir: &std::path::Path) {
    let record: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("session.json")).unwrap_or_default()).unwrap_or_default();
    let conversation = record["conversation"].clone();
    let prompt = serde_json::json!({"type": "user", "session_id": conversation, "message": {"role": "user", "content": "go"}});
    let answer = serde_json::json!({"type": "assistant", "session_id": conversation, "message": {"content": [{"type": "text", "text": "working"}]}});
    let mut stream = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("g1/stream.jsonl")).unwrap();
    writeln!(stream, "{prompt}\n{answer}").unwrap();
}
