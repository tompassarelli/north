//! The signals that say a host cannot take a new session, read from the host itself: its memory, its root disk and
//! its backlog, the machine-capacity admission queue and the load average against the host's threads. The backlog is
//! a placement signal only: a session already on a host is never held for it, and builds wait for their own pressure
//! target (`pressure_target`). Placement skips a host with a backlog at launch so the pile does not deepen while another
//! host idles, and alerts on a stalled fleet read the same signals through `Saturation::refusal`.
use super::admission::{self, Class, default_mib as default};
use super::disk_line::Line;
use north_base::platform::pushed_token::FRESH_SECONDS;

const DISK_FLOOR: &str = "build_disk_free_gib";
const COLD_BUILD: &str = "dispatch_cold_build_gib";
const KIB_PER_GIB: f64 = 1024.0 * 1024.0;
const KIB_PER_MIB: f64 = 1024.0;
const BACKLOG_MARK: &str = "#backlog";
const TOKEN_MARK: &str = "#token";

/// Jobs queued in machine-capacity admission at which a host takes no new session.
pub const QUEUED_LIMIT: u32 = 4;

/// Prints, one per line: `MemTotal` and `MemAvailable` in kB, then `df -Pk /`, then the backlog mark and
/// `north capacity host backlog`, which a host on an older runtime leaves empty, then the token mark and the seconds since the App token the laptop pushes to the host was written, which a host that mints its own token leaves empty.
pub const PROBE: &str = "awk '/^MemTotal:|^MemAvailable:/ {print $2}' /proc/meminfo; df -Pk / | tail -1; echo '#backlog'; \"$HOME/.local/state/north/current/tree/bin/north\" capacity host backlog 2>/dev/null; echo '#token'; t=\"$HOME/.local/state/north/gh-write-token\"; [ -f \"$t\" ] && echo $(($(date +%s) - $(stat -c %Y \"$t\"))); true";

/// The CPU stall, in avg10 basis points, of a host's unleased agent work past which a load over its threads keeps a new session off it.
pub const STALL_LIMIT: f64 = 1000.0;

/// What waits on a host: jobs queued in admission, the one-minute load average against its threads, and the CPU stall
/// of the agent work no lease caps, which a host on an older runtime does not report.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Backlog {
    pub queued: u32,
    pub load: f64,
    pub threads: u32,
    #[serde(default)]
    pub stall: Option<f64>,
}

impl Backlog {
    /// `QUEUED LOAD THREADS [STALL]` on one line, as `north capacity host backlog` prints it.
    pub fn parse(line: &str) -> Option<Self> {
        let mut words = line.split_whitespace();
        Some(Self { queued: words.next()?.parse().ok()?, load: words.next()?.parse().ok()?, threads: words.next()?.parse().ok()?, stall: words.next().and_then(|word| word.parse().ok()) })
    }

    /// A load over the host's threads keeps a session off only while the work a session runs beside stalls, since capped leases raise the load without taking a session's CPU.
    fn refusal(&self, host: &str) -> Option<String> {
        let queue = (self.queued >= QUEUED_LIMIT).then(|| format!("{} jobs queued in machine-capacity admission (limit {QUEUED_LIMIT})", self.queued));
        let stalled = self.stall.is_none_or(|stall| stall > STALL_LIMIT);
        let load = (self.load > f64::from(self.threads) && stalled).then(|| match self.stall {
            Some(stall) => format!("load {:.0} on {} threads with unleased agent work stalled {:.0}% of the time", self.load, self.threads, stall / 100.0),
            None => format!("load {:.0} on {} threads", self.load, self.threads),
        });
        let causes: Vec<String> = queue.into_iter().chain(load).collect();
        (!causes.is_empty()).then(|| format!("{host} skipped: {}", causes.join(", ")))
    }

    /// Builds the host's admission takes before its queue reaches the limit placement skips it at.
    pub fn build_room(&self) -> i64 {
        i64::from(QUEUED_LIMIT) - i64::from(self.queued)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Saturation {
    pub memory_total_mib: f64,
    pub memory_available_mib: f64,
    pub disk_free_gib: f64,
    /// The root disk's size, 0 when a cached reading predates it.
    #[serde(default)]
    pub disk_total_gib: f64,
    #[serde(default)]
    pub backlog: Option<Backlog>,
    /// Seconds since the App token pushed to the host was written, None when the host mints its own.
    #[serde(default)]
    pub token_age_s: Option<u64>,
    /// The host's `disk_free_gib` setting, None for the eviction line.
    #[serde(default)]
    pub disk_bound_gib: Option<f64>,
}

pub fn parse(text: &str) -> Option<Saturation> {
    let (readings, rest) = text.split_once(BACKLOG_MARK).unwrap_or((text, ""));
    let (backlog, token) = rest.split_once(TOKEN_MARK).unwrap_or((rest, ""));
    let mut lines = readings.lines();
    let total_kib: f64 = lines.next()?.trim().parse().ok()?;
    let available_kib: f64 = lines.next()?.trim().parse().ok()?;
    let mut df = lines.next()?.split_whitespace();
    let total: f64 = df.nth(1)?.parse().ok()?;
    let kib: f64 = df.nth(1)?.parse().ok()?;
    let backlog = backlog.lines().find_map(Backlog::parse);
    let token_age_s = token.lines().find_map(|line| line.trim().parse().ok());
    Some(Saturation { memory_total_mib: total_kib / KIB_PER_MIB, memory_available_mib: available_kib / KIB_PER_MIB, disk_free_gib: kib / KIB_PER_GIB, disk_total_gib: total / KIB_PER_GIB, backlog, token_age_s, disk_bound_gib: None })
}

impl Saturation {
    /// How many more sessions of the agent request fit in the host's available memory above the unattended floor; the load and stall checks in `refusal` still decide whether the host takes any.
    pub fn session_room(&self) -> i64 {
        let request = admission::dispatch_request_mib(Class::Agent);
        let spare = self.memory_available_mib - admission::memory_floor_mib(admission::Profile::Unattended, self.memory_total_mib);
        (((spare / request).floor() as i64).max(0)).min(self.disk_room())
    }

    pub fn bounded(self, disk_bound_gib: Option<f64>) -> Self {
        Self { disk_bound_gib, ..self }
    }

    /// The free GiB a root disk keeps: the host's `disk_free_gib`, else the eviction line the lease reaper reclaims under.
    pub fn disk_bound(&self) -> f64 {
        self.disk_bound_gib.unwrap_or_else(|| Line::eviction().floor_gib(self.disk_total_gib))
    }

    /// Cold builds the root disk takes above its bound, which a worker's first build writes into its lane.
    fn disk_room(&self) -> i64 {
        (((self.disk_free_gib - self.disk_bound()) / default(COLD_BUILD)).floor() as i64).max(0)
    }

    /// Why `host` takes no new session, the first of: a GitHub token past its trust window, too little available memory beyond a session's own request, a root disk under the build floor or under its disk bound, or queued jobs at the limit or load over its threads.
    pub fn refusal(&self, host: &str) -> Option<String> {
        if let Some(age) = self.token_age_s.filter(|age| *age >= FRESH_SECONDS.unsigned_abs()) {
            return Some(format!("{host} skipped: its GitHub token is {} min old, past its {} min trust, so a worker there cannot reach GitHub", age / 60, FRESH_SECONDS / 60));
        }
        let request = admission::dispatch_request_mib(Class::Agent);
        if admission::memory_short(self.memory_total_mib, self.memory_available_mib, request) {
            return Some(format!("{host} skipped: {:.1} GiB of memory available, too little beside a {:.1} GiB session", self.memory_available_mib / 1024.0, request / 1024.0));
        }
        let floor = default(DISK_FLOOR);
        if self.disk_free_gib < floor {
            return Some(format!("{host} skipped: {:.1} GiB free on the root disk, under {floor}", self.disk_free_gib));
        }
        let bound = self.disk_bound();
        if self.disk_free_gib < bound {
            return Some(format!("{host} skipped: {:.1} GiB free on the root disk, under its {bound:.0} GiB disk bound", self.disk_free_gib));
        }
        self.backlog?.refusal(host)
    }
}

impl From<Saturation> for north_strategy_types::placement::Saturation {
    fn from(reading: Saturation) -> Self {
        let backlog = reading.backlog.map(|backlog| north_strategy_types::placement::Backlog { queued: backlog.queued, load: backlog.load, threads: backlog.threads, stall: backlog.stall });
        Self { memory_total_mib: reading.memory_total_mib, memory_available_mib: reading.memory_available_mib, disk_free_gib: reading.disk_free_gib, disk_total_gib: reading.disk_total_gib, backlog, token_age_s: reading.token_age_s, disk_bound_gib: reading.disk_bound_gib }
    }
}

impl From<north_strategy_types::placement::Saturation> for Saturation {
    fn from(reading: north_strategy_types::placement::Saturation) -> Self {
        let backlog = reading.backlog.map(|backlog| Backlog { queued: backlog.queued, load: backlog.load, threads: backlog.threads, stall: backlog.stall });
        Self { memory_total_mib: reading.memory_total_mib, memory_available_mib: reading.memory_available_mib, disk_free_gib: reading.disk_free_gib, disk_total_gib: reading.disk_total_gib, backlog, token_age_s: reading.token_age_s, disk_bound_gib: reading.disk_bound_gib }
    }
}
