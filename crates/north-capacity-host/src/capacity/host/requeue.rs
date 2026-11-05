//! A queued wrapper that is the operator runtime follows `north runtime update`: it execs the new current runtime in place, so the same process, its ticket and its place in the queue are judged by the landed admission rules.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::machine::env;

const TICKET: &str = "AGENT_CAPACITY_RESUME_TICKET";
const QUEUED_AT: &str = "AGENT_CAPACITY_RESUME_QUEUED_AT";

/// The ticket and queue time this process carried across an exec, when its ticket still names this process.
pub struct Resumed {
    pub ticket: String,
    pub queued_at: f64,
}

pub fn resumed(root: &Path) -> Option<Resumed> {
    let ticket = env(TICKET)?;
    let queued_at = env(QUEUED_AT)?.parse().ok()?;
    let entry: Value = serde_json::from_str(&std::fs::read_to_string(root.join("queue").join(&ticket)).ok()?).ok()?;
    (entry.get("wrapperPid").and_then(Value::as_u64) == Some(u64::from(std::process::id()))).then_some(Resumed { ticket, queued_at })
}

/// Strips the resume variables from a command this wrapper starts, so a nested request never claims this wrapper's ticket.
pub fn forget(command: &mut std::process::Command) {
    command.env_remove(TICKET).env_remove(QUEUED_AT);
}

/// The operator runtime's binary, when this process is that binary; None for a development build, a fixture or AGENT_CAPACITY_NORTH.
pub struct Follow {
    link: PathBuf,
    started: PathBuf,
}

impl Follow {
    pub fn here() -> Option<Self> {
        if env("AGENT_CAPACITY_NORTH").is_some() {
            return None;
        }
        let link = Path::new(&env("HOME")?).join(".local/state/north/current/north");
        let started = link.canonicalize().ok()?;
        (std::env::current_exe().ok()?.canonicalize().ok()? == started).then_some(Self { link, started })
    }

    /// Execs the runtime the link now names with this process's arguments; returns only when the link is unchanged or the exec failed.
    pub fn follow(&self, ticket: &str, queued_at: f64) {
        let Ok(current) = self.link.canonicalize() else { return };
        if current == self.started {
            return;
        }
        let error = std::process::Command::new(&current).args(std::env::args_os().skip(1)).env(TICKET, ticket).env(QUEUED_AT, queued_at.to_string()).exec();
        eprintln!("machine-capacity: could not follow the runtime to {}: {error}", current.display());
    }
}
