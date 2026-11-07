use std::fmt;

use serde::{Deserialize, Serialize};

use crate::platform::host;

/// The process that holds or moved a state: its host, pid, start time since boot and the host's boot time, so a
/// reused pid or a reboot never passes for the original holder.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Owner {
    pub host: String,
    pub pid: u32,
    /// Field 22 of `/proc/PID/stat`: clock ticks from boot to the process's start.
    pub started: u64,
    pub booted_ms: u64,
}

impl Owner {
    pub fn current() -> Self {
        let pid = std::process::id();
        Self { host: host::name(), pid, started: started(pid).unwrap_or(0), booted_ms: host::booted_ms().unwrap_or(0) }
    }
}

impl fmt::Display for Owner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "pid {} on {}", self.pid, self.host)
    }
}

/// Whether an owner still runs; `None` when this host cannot tell, as for an owner on another host.
pub trait Liveness {
    fn alive(&self, owner: &Owner) -> Option<bool>;
}

/// Liveness read from this host's `/proc`.
pub struct Proc;

impl Liveness for Proc {
    fn alive(&self, owner: &Owner) -> Option<bool> {
        if owner.host != host::name() {
            return None;
        }
        if host::booted_ms().is_some_and(|booted| booted != owner.booted_ms) {
            return Some(false);
        }
        Some(started(owner.pid) == Some(owner.started))
    }
}

fn started(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()
}
