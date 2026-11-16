//! A host's live load as dispatch reads it: its cores, its memory and its one-minute run queue,
//! from `nproc`, `/proc/loadavg`, `/proc/meminfo`, two `/proc/stat` readings, two readings of the yielding slice's CPU time and `/proc/pressure/cpu` here or over ssh.
use north_base::platform::process::run_within;
use std::time::Duration;

const LIMIT: Duration = Duration::from_secs(15);
/// Microseconds in one `/proc/stat` tick at Linux's fixed USER_HZ of 100.
const TICK_MICROSECONDS: f64 = 10_000.0;
const YIELDING_USAGE: &str = "usage_usec ";

/// The probe: the yielding slice's CPU time, under its cgroup directory `yielding`, is read beside each `/proc/stat` line, so its share of the window is measured on the same 2 s.
pub fn probe(yielding: &str) -> String {
    format!("y=\"/sys/fs/cgroup/user.slice/user-$(id -u).slice/user@$(id -u).service/{yielding}/cpu.stat\"; nproc; cat /proc/loadavg /proc/meminfo; head -n1 /proc/stat; grep -s '^{YIELDING_USAGE}' \"$y\"; sleep 2; head -n1 /proc/stat; grep -s '^{YIELDING_USAGE}' \"$y\"; cat /proc/pressure/cpu 2>/dev/null; true")
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measure {
    pub cores: f64,
    pub memory_total_mib: f64,
    pub memory_available_mib: f64,
    /// The one-minute load average: runnable and waiting threads.
    pub load: f64,
    /// Cores the host can spare: the smaller of the cores idle over the 2 s probe window (idle plus waiting on disk) and the cores the last minute left free by CPU pressure, plus the cores the lander's yielding reruns and culprit searches took in the window, which give way to any session; None without CPU counters.
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
    let idle_cores = window(text).map(|window| cores * (window.idle.min(minute_share(text)) + window.yielding).clamp(0.0, 1.0));
    Some(Measure { cores, memory_total_mib: field("MemTotal:")?, memory_available_mib: field("MemAvailable:")?, load, idle_cores })
}

/// This host's measure when `here`, else the one its ssh destination reports, if it has one.
pub fn read(here: bool, ssh: Option<&str>, yielding: &str) -> Option<Measure> {
    let probe = probe(yielding);
    let output = match (here, ssh) {
        (true, _) => run_within("sh", &["-c", &probe], LIMIT),
        (false, Some(destination)) => run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", destination, &probe], LIMIT),
        (false, None) => None,
    }?;
    output.status.success().then(|| parse(&String::from_utf8_lossy(&output.stdout))).flatten()
}

/// Shares of all CPU time over the probe window: idle (idle plus waiting on disk), and taken by the yielding slice, 0 when either of its readings is missing.
struct Window {
    idle: f64,
    yielding: f64,
}

fn window(text: &str) -> Option<Window> {
    let mut counters = text.lines().filter(|line| line.starts_with("cpu ")).map(|line| line.split_whitespace().skip(1).take(8).map(|field| field.parse::<f64>().ok()).collect::<Option<Vec<f64>>>());
    let (before, after) = (counters.next()??, counters.next()??);
    if before.len() < 8 || after.len() < 8 {
        return None;
    }
    let (total, idle) = (after.iter().sum::<f64>() - before.iter().sum::<f64>(), after[3] + after[4] - before[3] - before[4]);
    let usage: Vec<f64> = text.lines().filter_map(|line| line.strip_prefix(YIELDING_USAGE)?.trim().parse().ok()).collect();
    let yielding = match usage[..] {
        [before, after] if after >= before => (after - before) / (total * TICK_MICROSECONDS),
        _ => 0.0,
    };
    (total > 0.0).then(|| Window { idle: (idle / total).clamp(0.0, 1.0), yielding: yielding.clamp(0.0, 1.0) })
}

/// The share of the cores the last minute left free; a guess that its share without CPU pressure equals that share of the cores, which a measured session count against pressure settles.
fn minute_share(text: &str) -> f64 {
    let pressure = text.lines().find(|line| line.starts_with("some ")).and_then(|line| line.split_whitespace().find_map(|field| field.strip_prefix("avg60=")?.parse::<f64>().ok()));
    pressure.map_or(1.0, |percent| (1.0 - percent / 100.0).clamp(0.0, 1.0))
}
