//! A host's live load as dispatch reads it: its cores, its memory and its one-minute run queue,
//! from `nproc`, `/proc/loadavg`, `/proc/meminfo` and two `/proc/stat` readings and `/proc/pressure/cpu` here or over ssh.
use crate::platform::process::run_within;
use std::time::Duration;

pub const PROBE: &str = "nproc; cat /proc/loadavg /proc/meminfo; head -n1 /proc/stat; sleep 2; head -n1 /proc/stat; cat /proc/pressure/cpu 2>/dev/null; true";
const LIMIT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measure {
    pub cores: f64,
    pub memory_total_mib: f64,
    pub memory_available_mib: f64,
    /// The one-minute load average: runnable and waiting threads.
    pub load: f64,
    /// Cores the host can spare: the smaller of the cores idle over the 2 s probe window (idle plus waiting on disk) and the cores the last minute left free by CPU pressure; None without CPU counters.
    pub idle_cores: Option<f64>,
}

/// The probe's output: `nproc`, then the loadavg line, then meminfo, then the two aggregate CPU counter lines.
pub fn parse(text: &str) -> Option<Measure> {
    let mut lines = text.lines();
    let cores = lines.next()?.trim().parse().ok()?;
    let load = lines.next()?.split_whitespace().next()?.parse().ok()?;
    let field = |name: &str| -> Option<f64> {
        let line = text.lines().find(|line| line.starts_with(name))?;
        let kib: f64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(kib / 1024.0)
    };
    Some(Measure { cores, memory_total_mib: field("MemTotal:")?, memory_available_mib: field("MemAvailable:")?, load, idle_cores: idle_cores(text, cores).map(|idle| idle.min(minute_cores(text, cores))) })
}

/// This host's measure when `here`, else the one its ssh destination reports, if it has one.
pub fn read(here: bool, ssh: Option<&str>) -> Option<Measure> {
    let output = match (here, ssh) {
        (true, _) => run_within("sh", &["-c", PROBE], LIMIT),
        (false, Some(destination)) => run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, PROBE], LIMIT),
        (false, None) => None,
    }?;
    output.status.success().then(|| parse(&String::from_utf8_lossy(&output.stdout))).flatten()
}

fn idle_cores(text: &str, cores: f64) -> Option<f64> {
    let mut counters = text.lines().filter(|line| line.starts_with("cpu ")).map(|line| line.split_whitespace().skip(1).take(8).map(|field| field.parse::<f64>().ok()).collect::<Option<Vec<f64>>>());
    let (before, after) = (counters.next()??, counters.next()??);
    if before.len() < 8 || after.len() < 8 {
        return None;
    }
    let (total, idle) = (after.iter().sum::<f64>() - before.iter().sum::<f64>(), after[3] + after[4] - before[3] - before[4]);
    (total > 0.0).then(|| cores * (idle / total).clamp(0.0, 1.0))
}

/// Cores the last minute left free; a guess that its share without CPU pressure equals that share of the cores, which a measured session count against pressure settles.
fn minute_cores(text: &str, cores: f64) -> f64 {
    let pressure = text.lines().find(|line| line.starts_with("some ")).and_then(|line| line.split_whitespace().find_map(|field| field.strip_prefix("avg60=")?.parse::<f64>().ok()));
    pressure.map_or(cores, |percent| cores * (1.0 - percent / 100.0).clamp(0.0, 1.0))
}
