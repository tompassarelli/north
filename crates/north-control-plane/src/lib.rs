//! The control plane's lease: the one row that names the host running the landers and the other control-plane
//! units, kept in its own database and read and changed through `north threads control-plane`.
use std::path::{Path, PathBuf};
use std::process::Command;

pub const SEED_HOLDER: &str = "whiterabbit";
pub const SEED_EPOCH: i64 = 1;
pub const LEASE_DATABASE: &str = "north/control-plane/lease.db";

pub fn database() -> PathBuf {
    north_base::platform::records_mirror::state_home().join(LEASE_DATABASE)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub holder: String,
    pub epoch: i64,
}

impl Lease {
    /// Parses the line `north threads control-plane read` prints: `holder=HOST epoch=N renewed_at=ISO`.
    pub fn parse(line: &str) -> Option<Self> {
        let mut holder = None;
        let mut epoch = None;
        for field in line.split_whitespace() {
            match field.split_once('=') {
                Some(("holder", value)) => holder = Some(value.to_owned()),
                Some(("epoch", value)) => epoch = value.parse().ok(),
                _ => {}
            }
        }
        Some(Self { holder: holder?, epoch: epoch? })
    }
}

/// A host name a lease may name: letters, digits and dashes.
pub fn is_host(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// A holder the ledger accepts: a host, or the `moving:OLD->NEW` a break-glass move writes between its two steps.
pub fn well_formed(holder: &str) -> bool {
    match moving(holder) {
        Some((from, to)) => is_host(from) && is_host(to),
        None => is_host(holder),
    }
}

/// The two hosts a move is under way between, when the lease reads `moving:OLD->NEW`.
pub fn moving(holder: &str) -> Option<(&str, &str)> {
    holder.strip_prefix("moving:")?.split_once("->")
}

/// This machine's short host name, the name a lease holder is compared with.
pub fn this_host() -> String {
    let name = north_base::platform::host::name();
    name.split('.').next().unwrap_or_default().to_owned()
}

/// Reads the lease through `north threads control-plane read`, run by the `north` binary at `north`.
pub fn read(north: &Path) -> Result<Lease, String> {
    let output = threads(north, &["control-plane", "read"])?;
    let text = String::from_utf8_lossy(&output.stdout);
    Lease::parse(text.trim()).ok_or_else(|| format!("north threads control-plane read printed no lease: {}", text.trim()))
}

/// Reads the lease and refuses unless this host holds it, so a lander that lost the lease pushes nothing to main.
pub fn holder_here(north: &Path) -> Result<Lease, String> {
    let lease = read(north).map_err(|why| format!("the control-plane lease could not be read, so this lander pushes nothing to main: {why}"))?;
    if lease.holder == this_host() {
        Ok(lease)
    } else {
        Err(format!("the control plane is held by {} at epoch {}, not {}: this lander pushes nothing to main", lease.holder, lease.epoch, this_host()))
    }
}

/// Changes the lease to `holder` when its epoch is still `epoch`, returning the row it wrote; a stale epoch is refused.
pub fn cas(north: &Path, epoch: i64, holder: &str) -> Result<Lease, String> {
    let epoch = epoch.to_string();
    let output = threads(north, &["control-plane", "cas", "--epoch", &epoch, "--holder", holder])?;
    let text = String::from_utf8_lossy(&output.stdout);
    Lease::parse(text.trim()).ok_or_else(|| format!("north threads control-plane cas printed no lease: {}", text.trim()))
}

fn threads(north: &Path, arguments: &[&str]) -> Result<std::process::Output, String> {
    let output = Command::new(north).arg("threads").args(arguments).output().map_err(|error| format!("cannot run {}: {error}", north.display()))?;
    if output.status.success() {
        Ok(output)
    } else {
        Err(format!("north threads {}: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()))
    }
}
