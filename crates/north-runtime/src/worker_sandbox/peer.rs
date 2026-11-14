//! The process a request to the outside broker comes from: its cgroup as `/proc/PID/cgroup` reads it, and the pids of it and of its ancestors, which an adopted window's record names (lane_grant.rs).

/// The caller of a broker request, read from its process: `cgroup` is `None` when `/proc/PID/cgroup` is unreadable.
#[derive(Debug)]
pub(super) struct Peer {
    cgroup: Option<String>,
    pids: Vec<u32>,
}

impl Peer {
    pub(super) fn of(pid: u32) -> Self {
        Self { cgroup: std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok(), pids: ancestry(pid) }
    }

    pub(super) fn cgroup(&self) -> Option<&str> {
        self.cgroup.as_deref()
    }

    pub(super) fn pids(&self) -> &[u32] {
        &self.pids
    }
}

impl From<Option<&str>> for Peer {
    fn from(cgroup: Option<&str>) -> Self {
        Self { cgroup: cgroup.map(str::to_owned), pids: Vec::new() }
    }
}

/// The pids of `pid` and of each ancestor above it, as the parents `/proc/PID/stat` names, up to init.
fn ancestry(pid: u32) -> Vec<u32> {
    let mut pids = vec![pid];
    while let Some(parent) = pids.last().copied().and_then(parent_of).filter(|parent| *parent > 1 && !pids.contains(parent)) {
        pids.push(parent);
    }
    pids
}

fn parent_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
impl Peer {
    pub(super) fn with_pids(cgroup: Option<&str>, pids: &[u32]) -> Self {
        Self { cgroup: cgroup.map(str::to_owned), pids: pids.to_vec() }
    }
}
