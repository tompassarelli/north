use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::json;

use crate::fleet::records::read_record;
use crate::fleet::runtime::SESSION_ENV;
use crate::foundation::clock::Timestamp;
use crate::hook_input::HookInput;

const RECORDS: &str = "agents/hooks/container-shell.jsonl";

/// Whether this hook's session was launched with `--container-shell`, so its shell tool runs in a task container and a command it runs there touches nothing of this host. The session's record says so, which its launch writes and no worker command can; nothing in the command or its environment does.
pub fn declared() -> bool {
    let session = std::env::var(SESSION_ENV).ok().filter(|session| !session.is_empty() && !session.contains('/'));
    session.zip(crate::fleet::registry::root_here().ok()).is_some_and(|(session, root)| declared_in(&root, &session))
}

pub(crate) fn declared_in(root: &Path, session: &str) -> bool {
    read_record(&root.join(session)).is_ok_and(|record| record.container_shell)
}

/// Appends what each guard that stood down for `input` would have said to the hooks' records under the state directory, where an eval's runner reads which guards did not apply to its session.
pub fn record(input: &HookInput, stood_down: &[String]) {
    let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).filter(|state| state.is_absolute()).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")));
    let Some(file) = state.filter(|_| !stood_down.is_empty()).map(|state| state.join(RECORDS)) else { return };
    let at = Timestamp::now().iso();
    let lines: String = stood_down.iter().map(|said| format!("{}\n", json!({"at": at, "tool": input.tool_name(), "command": input.command(), "said": said}))).collect();
    let _ = file.parent().map(std::fs::create_dir_all);
    if let Ok(mut records) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = records.write_all(lines.as_bytes());
    }
}
