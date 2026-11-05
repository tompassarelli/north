//! The signals that say a host cannot take a new session, read from the host itself: its memory, its root disk and
//! its backlog, the machine-capacity admission queue and the load average against the host's threads. The backlog is
//! a placement signal only: a session already on a host is never held for it, and builds wait for their own pressure
//! target (`pressure_target`). Placement skips a host with a backlog at launch so the pile does not deepen while another
//! host idles, and alerts on a stalled fleet read the same signals through `Saturation::refusal`.
use super::admission::{self, Class, default_mib as default};

const DISK_FLOOR: &str = "build_disk_free_gib";
const KIB_PER_GIB: f64 = 1024.0 * 1024.0;
const KIB_PER_MIB: f64 = 1024.0;
const BACKLOG_MARK: &str = "#backlog";

/// Jobs queued in machine-capacity admission at which a host takes no new session.
pub const QUEUED_LIMIT: u32 = 4;

/// Prints, one per line: `MemTotal` and `MemAvailable` in kB, then `df -Pk /`, then the backlog mark and
/// `north capacity host backlog`, which a host on an older runtime leaves empty.
pub const PROBE: &str = "awk '/^MemTotal:|^MemAvailable:/ {print $2}' /proc/meminfo; df -Pk / | tail -1; echo '#backlog'; \"$HOME/.local/state/north/current/tree/bin/north\" capacity host backlog 2>/dev/null; true";

/// What waits on a host: jobs queued in admission and the one-minute load average against its threads.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Backlog {
    pub queued: u32,
    pub load: f64,
    pub threads: u32,
}

impl Backlog {
    /// `QUEUED LOAD THREADS` on one line, as `north capacity host backlog` prints it.
    pub fn parse(line: &str) -> Option<Self> {
        let mut words = line.split_whitespace();
        Some(Self { queued: words.next()?.parse().ok()?, load: words.next()?.parse().ok()?, threads: words.next()?.parse().ok()? })
    }

    fn refusal(&self, host: &str) -> Option<String> {
        let queue = (self.queued >= QUEUED_LIMIT).then(|| format!("{} jobs queued in machine-capacity admission (limit {QUEUED_LIMIT})", self.queued));
        let load = (self.load > f64::from(self.threads)).then(|| format!("load {:.0} on {} threads", self.load, self.threads));
        let causes: Vec<String> = queue.into_iter().chain(load).collect();
        (!causes.is_empty()).then(|| format!("{host} skipped: {}", causes.join(", ")))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Saturation {
    pub memory_total_mib: f64,
    pub memory_available_mib: f64,
    pub disk_free_gib: f64,
    #[serde(default)]
    pub backlog: Option<Backlog>,
}

pub fn parse(text: &str) -> Option<Saturation> {
    let (readings, backlog) = text.split_once(BACKLOG_MARK).unwrap_or((text, ""));
    let mut lines = readings.lines();
    let total_kib: f64 = lines.next()?.trim().parse().ok()?;
    let available_kib: f64 = lines.next()?.trim().parse().ok()?;
    let kib: f64 = lines.next()?.split_whitespace().nth(3)?.parse().ok()?;
    let backlog = backlog.lines().find_map(Backlog::parse);
    Some(Saturation { memory_total_mib: total_kib / KIB_PER_MIB, memory_available_mib: available_kib / KIB_PER_MIB, disk_free_gib: kib / KIB_PER_GIB, backlog })
}

impl Saturation {
    /// Why `host` takes no new session, the first of: too little available memory beyond a session's own request, a root disk under the build floor, or queued jobs at the limit or load over its threads.
    pub fn refusal(&self, host: &str) -> Option<String> {
        let request = admission::dispatch_request_mib(Class::Agent);
        if admission::memory_short(self.memory_total_mib, self.memory_available_mib, request) {
            return Some(format!("{host} skipped: {:.1} GiB of memory available, too little beside a {:.1} GiB session", self.memory_available_mib / 1024.0, request / 1024.0));
        }
        let floor = default(DISK_FLOOR);
        if self.disk_free_gib < floor {
            return Some(format!("{host} skipped: {:.1} GiB free on the root disk, under {floor}", self.disk_free_gib));
        }
        self.backlog?.refusal(host)
    }
}

impl From<Saturation> for north_strategy_types::placement::Saturation {
    fn from(reading: Saturation) -> Self {
        let backlog = reading.backlog.map(|backlog| north_strategy_types::placement::Backlog { queued: backlog.queued, load: backlog.load, threads: backlog.threads });
        Self { memory_total_mib: reading.memory_total_mib, memory_available_mib: reading.memory_available_mib, disk_free_gib: reading.disk_free_gib, backlog }
    }
}

impl From<north_strategy_types::placement::Saturation> for Saturation {
    fn from(reading: north_strategy_types::placement::Saturation) -> Self {
        let backlog = reading.backlog.map(|backlog| Backlog { queued: backlog.queued, load: backlog.load, threads: backlog.threads });
        Self { memory_total_mib: reading.memory_total_mib, memory_available_mib: reading.memory_available_mib, disk_free_gib: reading.disk_free_gib, backlog }
    }
}
