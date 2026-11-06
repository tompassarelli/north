use std::fs;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Account;
use crate::fleet::runtime::WindowInbox;

const SESSIONS: &str = "sessions";
const DEADLINE: Duration = Duration::from_secs(5);
const DRAIN: Duration = Duration::from_millis(500);

pub(super) fn find(accounts: &[Account], conversation: &str) -> Option<WindowInbox> {
    accounts.iter().find_map(|account| in_profile(&account.label, &account.home.join(SESSIONS), conversation))
}

fn in_profile(account: &str, sessions: &Path, conversation: &str) -> Option<WindowInbox> {
    fs::read_dir(sessions).ok()?.flatten().find_map(|entry| {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            return None;
        }
        let record: Value = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
        if record["sessionId"].as_str() != Some(conversation) {
            return None;
        }
        let pid = u32::try_from(record["pid"].as_u64()?).ok()?;
        if !Path::new(&format!("/proc/{pid}")).exists() {
            return None;
        }
        let socket = PathBuf::from(record["messagingSocketPath"].as_str()?);
        Some(WindowInbox { account: account.to_owned(), pid, socket, token: peer_token(sessions, pid)? })
    })
}

fn peer_token(sessions: &Path, pid: u32) -> Option<String> {
    let prefix = format!("{pid}.");
    fs::read_dir(sessions).ok()?.flatten().find_map(|entry| {
        let name = entry.file_name().into_string().ok()?;
        let hash = name.strip_prefix(&prefix)?.strip_suffix(".key")?;
        if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let key: Value = serde_json::from_slice(&fs::read(entry.path()).ok()?).ok()?;
        key["peerToken"].as_str().map(str::to_owned)
    })
}

pub(super) fn tell(inbox: &WindowInbox, text: &str) -> NorthResult<()> {
    let refused = |error: std::io::Error| NorthError::State(format!("the window inbox of process {} at {} refused the message: {error}", inbox.pid, inbox.socket.display()));
    let mut stream = UnixStream::connect(&inbox.socket).map_err(refused)?;
    stream.set_write_timeout(Some(DEADLINE)).map_err(refused)?;
    stream.set_read_timeout(Some(DRAIN)).map_err(refused)?;
    let lines = format!("{}\n{}\n", json!({"type": "auth", "token": inbox.token}), json!({"type": "user", "message": {"role": "user", "content": text}}));
    stream.write_all(lines.as_bytes()).map_err(refused)?;
    stream.shutdown(Shutdown::Write).map_err(refused)?;
    let _ = stream.read_to_end(&mut Vec::new());
    Ok(())
}
