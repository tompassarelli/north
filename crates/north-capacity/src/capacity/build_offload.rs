//! Whether a heavy build this host would defer runs on another host instead, and which one.

pub const REASON: &str = "OFFLOAD";
pub const REMOTE_MARK: &str = "NORTH_BUILD_OFFLOADED";
pub const PROBE_LIMIT_SECONDS: u64 = 5;
pub const UNREACHABLE_RETRY_FACTOR: f64 = 20.0;

/// A host's admission state as `north capacity host status` reports it: what its own admission would let a build start against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Admission {
    pub limit_cpus: f64,
    pub committed_cpus: f64,
    pub cpu_pressure_percent: f64,
    pub pressure_high_percent: f64,
    pub land_check_waiting: bool,
}

impl Admission {
    pub fn from_status(status: &serde_json::Value) -> Option<Self> {
        let number = |key: &str| status.get(key)?.as_f64();
        let band = match status.get("profile")?.as_str()? {
            "attended" => "admit_psi_attended_high",
            "unattended" => "admit_psi_unattended_high",
            _ => return None,
        };
        Some(Self { limit_cpus: number("aggregateCpuLimit")?, committed_cpus: number("committedBatchCpus")?, cpu_pressure_percent: number("cpuSomeAvg10")?, pressure_high_percent: super::admission::default_mib(band), land_check_waiting: status.get("landCheckWaiting").and_then(serde_json::Value::as_bool).unwrap_or(false) })
    }

    /// Why a build declaring `needs` CPUs would wait on this host's admission, with `reserved` CPUs counted as taken by builds already on their way; the host's load average, which its CI farm fills inside quota, never counts.
    pub fn shortfall(&self, needs: f64, reserved: f64) -> Option<String> {
        if self.cpu_pressure_percent >= self.pressure_high_percent {
            return Some(format!("CPU pressure {:.0}% at the {:.0}% admission mark", self.cpu_pressure_percent, self.pressure_high_percent));
        }
        let free = self.limit_cpus - self.committed_cpus - reserved;
        (free < needs).then(|| format!("{free:.1} cores free of {}, needs {needs}", self.limit_cpus))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Job<'a> {
    pub owner: &'a str,
    pub timeout_seconds: u64,
    pub command: &'a [String],
}

#[derive(Clone, Debug, PartialEq)]
pub enum Room {
    Free,
    Short(String),
    Unreachable,
}

#[derive(Clone, Copy, Debug)]
pub struct Candidate<'a> {
    pub class: &'a str,
    pub landing: bool,
    pub need: Option<&'a str>,
    pub deferred: bool,
    pub agent: bool,
    pub offloaded_already: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    Local,
    Offload { host: String },
    Declined(Declined),
}

/// Why a deferred heavy build stays queued here although an offload host is configured.
#[derive(Clone, Debug, PartialEq)]
pub enum Declined {
    NoRoom(Vec<String>),
    Unreachable(Vec<String>),
    Refused { class: String, detail: String },
    Failed(String),
}

impl Declined {
    pub fn reason(&self) -> String {
        match self {
            Self::NoRoom(_) => "no room".to_owned(),
            Self::Unreachable(_) => "unreachable".to_owned(),
            Self::Refused { class, .. } => format!("refused: {class}"),
            Self::Failed(_) => "failed".to_owned(),
        }
    }

    pub fn detail(&self) -> String {
        match self {
            Self::NoRoom(hosts) | Self::Unreachable(hosts) => hosts.join("; "),
            Self::Refused { detail, .. } => detail.clone(),
            Self::Failed(why) => why.clone(),
        }
    }

    /// How long a queued build waits before offering itself again: a decline that found every host out of reach waits `UNREACHABLE_RETRY_FACTOR` times as long, since only a changed path to a host can end it.
    pub fn retry_milliseconds(&self, base: f64) -> f64 {
        if matches!(self, Self::Unreachable(_)) { base * UNREACHABLE_RETRY_FACTOR } else { base }
    }

    pub fn refused(class: &str, detail: String) -> Self {
        Self::Refused { class: class.to_owned(), detail }
    }

    /// True when this decline names a different reason than the one last printed, which it then becomes.
    pub fn changes(&self, last: &mut Option<String>) -> bool {
        let reason = self.reason();
        let fresh = last.as_deref() != Some(reason.as_str());
        *last = Some(reason);
        fresh
    }
}

/// The class that keeps a deferred heavy build on this host: a local need (GPU, native, Warcraft client, private asset) names its host by `serves`, and the landing check has its own offload.
pub fn refusal(candidate: &Candidate) -> Option<Declined> {
    if candidate.landing {
        return Some(Declined::refused("landing", String::new()));
    }
    if candidate.offloaded_already {
        return Some(Declined::refused("already offloaded", String::new()));
    }
    candidate.need.filter(|need| !need.is_empty()).map(|need| Declined::refused(need, String::new()))
}

/// A deferred heavy build moves when a host has room, and an agent's heavy build is offered to the hosts first even when this host would admit it, so the laptop builds only what no build host has room for.
pub fn wants_offload(candidate: &Candidate) -> bool {
    candidate.class == "heavy" && (candidate.deferred || candidate.agent)
}

pub fn first_free(hosts: impl IntoIterator<Item = (String, Room)>) -> Result<String, Declined> {
    let (mut short, mut dark) = (Vec::new(), Vec::new());
    for (name, room) in hosts {
        match room {
            Room::Free => return Ok(name),
            Room::Short(why) => short.push(format!("{name}: {why}")),
            Room::Unreachable => dark.push(format!("{name}: unreachable or unreadable over ssh")),
        }
    }
    if short.is_empty() {
        return Err(Declined::Unreachable(dark));
    }
    short.extend(dark);
    Err(Declined::NoRoom(short))
}

/// The probe runs only for a deferred heavy request this host may move, so a local admit, a pinned box or a host with no offload host costs no ssh.
pub fn route(candidate: &Candidate, rooms: impl FnOnce() -> Vec<(String, Room)>) -> Route {
    if !wants_offload(candidate) {
        return Route::Local;
    }
    if let Some(refused) = refusal(candidate) {
        return Route::Declined(refused);
    }
    let rooms = rooms();
    if rooms.is_empty() {
        return Route::Local;
    }
    first_free(rooms).map_or_else(Route::Declined, |host| Route::Offload { host })
}

/// Files whose extension marks a private or game asset, which never leave the laptop in a lane's uncommitted diff.
pub const PRIVATE_EXTENSIONS: [&str; 12] = ["w3x", "w3m", "w3n", "mpq", "blp", "mdx", "mdl", "casc", "dds", "tga", "wav", "mp3"];
pub const MAX_DIFF_BYTES: usize = 16 * 1024 * 1024;

pub fn ships(changed: &[&str], diff_bytes: usize) -> Result<(), Declined> {
    if diff_bytes > MAX_DIFF_BYTES {
        return Err(Declined::refused("oversized diff", format!("the uncommitted diff is {diff_bytes} bytes, over the {MAX_DIFF_BYTES} byte limit for shipping a lane")));
    }
    let private = changed.iter().find(|path| path.rsplit_once('.').is_some_and(|(_, extension)| PRIVATE_EXTENSIONS.contains(&extension.to_ascii_lowercase().as_str())));
    private.map_or(Ok(()), |path| Err(Declined::refused("private asset", (*path).to_owned())))
}

/// Runs a candidate's job on another host and returns its exit code, calling the notifier with the host once the command starts there; Err leaves it to the local path, with the reason when an offload was declined.
pub type Offloader = fn(&Candidate, &Job, &dyn Fn(&str)) -> Result<u8, Option<Declined>>;

/// The host keeps its own trees at the lane's and the main checkout's absolute paths, so a command that names either would run there against them instead of the shipped lane.
pub fn names_a_path_here(command: &[String], paths: &[&str]) -> Result<(), Declined> {
    let found = command.iter().find_map(|word| paths.iter().find(|path| word.contains(**path)));
    found.map_or(Ok(()), |path| Err(Declined::refused("absolute path", format!("the command names {path}, which the host keeps its own checkout at; use paths relative to the lane"))))
}

pub const SHIP_SECONDS_ENV: &str = "NORTH_OFFLOAD_SHIP_SECONDS";
pub const SLOT_TARGET_ENV: &str = "NORTH_OFFLOAD_SLOT_TARGET";
pub const SLOT_HEAD_ENV: &str = "NORTH_OFFLOAD_SLOT_HEAD";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheState {
    Warm,
    Cold,
}

impl CacheState {
    /// Warm when the lane's slot still held its `target/` and a previous head, so the build reuses compiled artifacts; a fresh slot or a swept one is cold.
    pub fn of(target_existed: bool, last_head: Option<&str>) -> Self {
        if target_existed && last_head.is_some_and(|head| !head.is_empty()) { Self::Warm } else { Self::Cold }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Warm => "warm",
            Self::Cold => "cold",
        }
    }
}

/// What an offloaded run paid before its command started on the host that ran it: the lane's ship and checkout, and the slot's cache state.
#[derive(Clone, Debug, PartialEq)]
pub struct ShipCost {
    pub host: String,
    pub ship_seconds: f64,
    pub cache_state: CacheState,
    pub slot_head: Option<String>,
}

impl ShipCost {
    /// Read from the variables the host's offload script sets for the run it starts; None for a run that was not offloaded.
    pub fn read(env: impl Fn(&str) -> Option<String>, host: &str) -> Option<Self> {
        let ship_seconds: f64 = env(SHIP_SECONDS_ENV)?.trim().parse().ok().filter(|seconds: &f64| seconds.is_finite() && *seconds >= 0.0)?;
        let slot_head = env(SLOT_HEAD_ENV).filter(|head| !head.is_empty());
        let target_existed = env(SLOT_TARGET_ENV).as_deref() == Some("1");
        Some(Self { host: host.to_owned(), ship_seconds, cache_state: CacheState::of(target_existed, slot_head.as_deref()), slot_head })
    }

    /// The keys an offloaded run's record carries beyond a local run's; `host` is where the run executed, so remote rows feed placement and never the laptop's sizing.
    pub fn fields(&self) -> Vec<(&'static str, serde_json::Value)> {
        let seconds = (self.ship_seconds * 100.0).round() / 100.0;
        let mut fields = vec![("host", self.host.clone().into()), ("shipSeconds", serde_json::json!(seconds)), ("cacheState", self.cache_state.as_str().into())];
        if let Some(head) = &self.slot_head {
            fields.push(("slotHead", head.clone().into()));
        }
        fields
    }
}

pub fn median(values: &[f64]) -> Option<f64> {
    let mut sorted: Vec<f64> = values.iter().copied().filter(|value| value.is_finite()).collect();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    match sorted.len() {
        0 => None,
        length if length % 2 == 1 => Some(sorted[middle]),
        _ => Some((sorted[middle - 1] + sorted[middle]) / 2.0),
    }
}
