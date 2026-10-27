//! A host's live load as dispatch reads it: its cores, its memory and its one-minute run queue,
//! from `nproc`, `/proc/loadavg` and `/proc/meminfo` here or over ssh.
use crate::platform::process::run_within;
use std::time::Duration;

const PROBE: &str = "nproc; cat /proc/loadavg /proc/meminfo";
const LIMIT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Measure {
    pub cores: f64,
    pub memory_total_mib: f64,
    pub memory_available_mib: f64,
    /// The one-minute load average: runnable and waiting threads.
    pub load: f64,
}

/// The probe's output: `nproc`, then the loadavg line, then meminfo.
pub(crate) fn parse(text: &str) -> Option<Measure> {
    let mut lines = text.lines();
    let cores = lines.next()?.trim().parse().ok()?;
    let load = lines.next()?.split_whitespace().next()?.parse().ok()?;
    let field = |name: &str| -> Option<f64> {
        let line = text.lines().find(|line| line.starts_with(name))?;
        let kib: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kib / 1024.0)
    };
    Some(Measure { cores, memory_total_mib: field("MemTotal:")?, memory_available_mib: field("MemAvailable:")?, load })
}

/// This host's measure when `here`, else the one its ssh destination reports, if it has one.
pub(crate) fn read(here: bool, ssh: Option<&str>) -> Option<Measure> {
    let output = match (here, ssh) {
        (true, _) => run_within("sh", &["-c", PROBE], LIMIT),
        (false, Some(destination)) => run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, PROBE], LIMIT),
        (false, None) => None,
    }?;
    output.status.success().then(|| parse(&String::from_utf8_lossy(&output.stdout))).flatten()
}
