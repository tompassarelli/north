//! What a host sends admission: its profile and class, the resources it leases and requests, and the
//! holds, clients and clocks the request meets.

use serde::{Deserialize, Serialize};

use super::super::client_reserve;
use super::super::native_hold::{LeaseFact, PairLease, QueueEntry, QuietNative};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    Attended,
    Unattended,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    Agent,
    Native,
    Moderate,
    Heavy,
    Exclusive,
    Gpu,
    Critical,
    Tom,
    Runtime,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    pub profile: Profile,
    pub class: Class,
    pub cores: u32,
    /// The agent.slice CPU ceiling, when its cpu.max sets one.
    #[serde(default)]
    pub slice_cpus: Option<f64>,
    #[serde(rename = "memoryTotalMiB")]
    pub memory_total_mib: f64,
    #[serde(rename = "memoryAvailableMiB")]
    pub memory_available_mib: f64,
    pub cpu_some_avg10_basis_points: u32,
    pub protected_cpu_some_avg10_basis_points: u32,
    /// The whole host's CPU stall, its batch slices included, which `cpu_some_avg10_basis_points` leaves out.
    #[serde(default)]
    pub host_cpu_some_avg10_basis_points: Option<u32>,
    #[serde(default)]
    pub cpu_pressure_measured: bool,
    #[serde(rename = "leasedMemoryMiB")]
    pub leased_memory_mib: f64,
    pub leased_batch_cpus: f64,
    #[serde(default)]
    pub unleased_heavy_cores: f64,
    pub leased_native_cpus: f64,
    pub requested_cpus: f64,
    #[serde(rename = "requestedMemoryMiB")]
    pub requested_memory_mib: f64,
    pub peer_batch_runs: u32,
    pub peer_exclusive_runs: u32,
    pub unbounded_runs: u32,
    pub queued_ahead: u32,
    pub exclusive_waiting: u32,
    #[serde(default)]
    pub before_drain: bool,
    pub gpu_busy_percent: u32,
    pub gpu_clients: u32,
    #[serde(default)]
    pub host_warcraft_clients: Option<u32>,
    #[serde(default, rename = "bar1FreeMiB")]
    pub bar1_free_mib: Option<f64>,
    #[serde(default, rename = "bar1TimedOut")]
    pub bar1_timed_out: bool,
    #[serde(default, rename = "vramFreeMiB")]
    pub vram_free_mib: Option<f64>,
    #[serde(default, rename = "hostBar1MarginMiB")]
    pub host_bar1_margin_mib: Option<u32>,
    #[serde(default)]
    pub host_gpu_slots: Option<u32>,
    pub new_clients: u32,
    #[serde(default)]
    pub new_desktops: u32,
    pub gpu_runs: u32,
    pub native_waiting: u32,
    #[serde(default)]
    pub quiet_native: Vec<QuietNative>,
    #[serde(default)]
    pub alone: bool,
    #[serde(default)]
    pub alone_queued_ahead: u32,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub pair: Option<u32>,
    #[serde(default)]
    pub pair_leases: Vec<PairLease>,
    #[serde(default)]
    pub leases: Vec<LeaseFact>,
    #[serde(default)]
    pub queue: Vec<QueueEntry>,
    #[serde(default)]
    pub now: f64,
    pub beside_native: bool,
    #[serde(default)]
    pub quiet: bool,
    #[serde(default)]
    pub exclusive_leases: Vec<String>,
    #[serde(skip)]
    pub within_exclusive: bool,
    #[serde(default)]
    pub gaming: bool,
    #[serde(default)]
    pub gaming_class: Option<Class>,
    #[serde(default)]
    pub game_reserve_agent_cpus: Option<u32>,
    #[serde(default)]
    pub queued_ahead_tickets: Vec<AheadTicket>,
    #[serde(default)]
    pub queued_ahead_new_clients: Vec<u32>,
    #[serde(default)]
    pub run_queue_load: Option<f64>,
    #[serde(default)]
    pub need: Option<String>,
    #[serde(default, rename = "diskFreeGiB")]
    pub disk_free_gib: Option<f64>,
    #[serde(default)]
    pub ticket: Option<std::path::PathBuf>,
    #[serde(default)]
    pub requester_lease: Option<String>,
    #[serde(default)]
    pub land_check_waiting: bool,
    #[serde(skip)]
    pub holds_offload_slot: bool,
    /// Whether this is a runtime canary's offloaded land check, which starts from its own reserve beside the land checks.
    #[serde(default)]
    pub canary: bool,
    /// Whether other hosts send builds here (`[hosts.NAME] offload_builds`).
    #[serde(skip)]
    pub build_host: bool,
    #[serde(skip)]
    pub client_hold: client_reserve::ClientHold,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AheadTicket {
    pub requested_class: Option<Class>,
    pub requested_cpus: Option<f64>,
    #[serde(rename = "requestedMemoryMiB")]
    pub requested_memory_mib: Option<f64>,
    pub queued_seconds: f64,
}
