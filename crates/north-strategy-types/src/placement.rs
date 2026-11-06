//! Placement of one `north fleet launch` or `revive` of a Claude box: every input placement reads,
//! gathered before it decides, and where it put the box.
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Case {
    pub at: f64,
    pub launch: Launch,
    pub brief: String,
    pub config: Config,
    pub credit: Credit,
    pub usage: Usage,
    /// `accounts.NAME.target.by` from pace-control.json, for each account that has one.
    pub pace: BTreeMap<String, String>,
    pub roster: BTreeMap<String, Standing>,
    pub machines: Vec<Machine>,
    pub here: Here,
    /// The lowest GitHub rate limit the broker last saw, None when it has seen none in a live window.
    #[serde(default)]
    pub github: Option<crate::github::Quota>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Launch {
    pub name: String,
    pub account: String,
    pub fresh: bool,
    pub cwd: Option<String>,
    pub named_machine: Option<String>,
    pub overrides: Overrides,
    pub provider: String,
    pub verification: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Overrides {
    pub local: Option<String>,
    pub pin: Option<String>,
    pub duplicate: Option<String>,
    pub tier: Option<String>,
}

/// The parts of South's orchestration.toml placement reads, and the sha256 of the whole file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub sha256: String,
    pub order: Vec<String>,
    pub public_repos: Vec<String>,
    pub private_repos: Vec<String>,
    pub cloud_credit: Option<String>,
    /// `[hosts.NAME]` tables in file order, which orders the hosts `[placement] order` leaves out.
    pub hosts: Vec<Host>,
    /// `[accounts.claude] plans`: label to plan name.
    pub plans: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Host {
    pub name: String,
    pub ssh: Option<String>,
    pub serves: Vec<String>,
    pub pushes: Option<Vec<String>>,
    pub reaches: Option<Vec<String>>,
    pub resident_cap: Option<i64>,
    pub launches: bool,
}

/// The `costs.toml` service `cloud_credit` names, and the spend readout beside it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Credit {
    pub configured: bool,
    pub expires: Option<f64>,
    pub initial: f64,
    pub spent: Option<f64>,
    pub read_at: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub sha256: String,
    /// Seconds between the file's last write and `at`.
    pub age_s: Option<f64>,
    pub controls: BTreeMap<String, Vec<Window>>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Window {
    pub window: Option<String>,
    pub used_pct: Option<f64>,
    pub resets_at: Option<String>,
    pub reset_h: Option<f64>,
    pub burn_pct_h: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    Listed,
    Duplicate(String),
    Foreign(Vec<String>),
    Unknown,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Machine {
    pub name: String,
    /// Resident CLIs the machine reports, None when unreachable; `hosted` is added to it.
    pub live: Option<i64>,
    pub accounts: Vec<String>,
    pub saturation: Option<Saturation>,
    pub saturation_age_s: Option<f64>,
    /// Fleet sessions this machine runs there.
    pub hosted: i64,
    /// Whether the launch directory exists there; None when the launch names none.
    pub has_cwd: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Saturation {
    pub memory_total_mib: f64,
    pub memory_available_mib: f64,
    pub disk_free_gib: f64,
    pub backlog: Option<Backlog>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Backlog {
    pub queued: u32,
    pub load: f64,
    pub threads: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Here {
    pub name: String,
    pub gaming: bool,
    /// The settled battery percent while discharging, None on line power.
    pub battery: Option<u8>,
    pub playtest_hold_until: Option<u64>,
    pub saturation: Option<Saturation>,
    /// The spawn gate's inputs, None when the gate is not read.
    pub spawn_cpu: Option<SpawnCpu>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SpawnCpu {
    pub provisioned: f64,
    pub limit: f64,
    pub protected: f64,
    /// Local workers running, read only when the CPU line refuses.
    pub local_workers: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub target: Target,
    pub machine: Option<String>,
    pub outcome: Outcome,
    /// The verdict lines `--report` prints, the summary first.
    pub lines: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Cloud { account: String, host: String, credit: String },
    Account(String),
    Held(String),
    Unserved(String),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Placed { account: Option<String>, note: Option<String>, machine: Option<String>, need: Option<String> },
    Refused { kind: Refusal, why: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Refusal {
    Saturated,
    Exhausted,
    Final,
}
