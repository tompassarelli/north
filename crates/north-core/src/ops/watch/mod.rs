mod gather;
mod host_facts;
mod probes;
mod shells;
mod waste;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod waste_tests;

pub use probes::PROBES;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum Severity {
    Info,
    Warn,
    Page,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Page => "page",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Alert {
    pub key: String,
    pub severity: Severity,
    pub repo: Option<String>,
    pub owner: Option<String>,
    pub summary: String,
    pub since: u64,
    pub repeat_secs: Option<u64>,
}

impl Alert {
    pub fn line(&self) -> String {
        let repo = self.repo.as_deref().map(|repo| format!(" repo={repo}")).unwrap_or_default();
        let owner = self.owner.as_deref().map(|owner| format!(" owner={owner}")).unwrap_or_default();
        format!("{} {}{repo}{owner}: {}", self.severity.label(), self.key, self.summary)
    }
}

pub type Probe = fn(&WatchContext) -> Vec<Alert>;

#[derive(Clone, Debug, Default)]
pub struct WatchContext {
    pub now: u64,
    pub hosts: Vec<HostView>,
    pub sessions: Vec<SessionView>,
    pub cloud: CloudView,
    pub subagents: Vec<SubagentView>,
    pub waits: Vec<WaitView>,
    pub runtime: RuntimeView,
}

#[derive(Clone, Debug, Default)]
pub struct HostView {
    pub name: String,
    pub local: bool,
    pub down_since: Option<u64>,
    pub capacity: Option<CapacityView>,
    pub waste: Option<WasteView>,
}

#[derive(Clone, Debug, Default)]
pub struct WasteView {
    pub cores: f64,
    pub busy_cores: f64,
    pub memory_total_mib: f64,
    pub memory_available_mib: f64,
    pub committed_cpus: f64,
    pub cpu_limit: f64,
    pub stale_leases: Vec<String>,
    pub game_reserve_on: bool,
    pub game_seen: Option<u64>,
    pub slot_pools: Vec<SlotPool>,
    pub offloads_waiting: Vec<u64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SlotPool {
    pub name: String,
    pub held: usize,
    pub total: usize,
}

#[derive(Clone, Debug, Default)]
pub struct CapacityView {
    pub queued: Vec<Ticket>,
    pub cpu_some_avg10: f64,
    pub pressure_target: f64,
    pub pressure_since: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct Ticket {
    pub owner: String,
    pub class: String,
    pub queued_since: u64,
    pub wait_p90_secs: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct SessionView {
    pub name: String,
    pub status: String,
    pub parent: Option<String>,
    pub repo: Option<String>,
    pub working_since: u64,
    pub last_event: Option<u64>,
    pub eta_min: Option<u64>,
    pub waiting: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CloudView {
    pub sessions: Vec<CloudSession>,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct CloudSession {
    pub id: String,
    pub status: String,
    pub updated: u64,
    pub title: String,
}

#[derive(Clone, Debug, Default)]
pub struct SubagentView {
    pub agent: String,
    pub session: String,
    pub owner: Option<String>,
    pub repo: Option<String>,
    pub last_event: u64,
    pub shell_finished: Option<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct WaitView {
    pub session: String,
    pub run: String,
    pub repo: Option<String>,
    pub since: u64,
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeView {
    pub abandoned: Vec<u64>,
    pub live: Option<String>,
    pub rust_change_since: Option<u64>,
}

pub fn pass(context: &WatchContext) -> Vec<Alert> {
    PROBES.iter().flat_map(|probe| probe(context)).collect()
}

const USAGE: &str = "usage: north watch --once: run every work probe once and print one line per alert";

pub fn run(arguments: &[String]) -> u8 {
    if arguments != ["--once"] {
        eprintln!("{USAGE}");
        return 2;
    }
    let context = gather::context();
    for alert in pass(&context) {
        println!("{}", alert.line());
    }
    0
}
