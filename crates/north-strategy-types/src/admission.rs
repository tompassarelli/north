//! One admission call: the request part of the facts, the hash of the host snapshot it was decided
//! on, and the reason admission gave. Field names follow the facts JSON the capacity host writes.
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Case {
    pub request: Request,
    /// The hash of the snapshot line this call was decided on.
    pub snapshot: String,
    pub settle_seconds: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub now: f64,
    pub class: Class,
    pub gaming_class: Option<Class>,
    pub requested_cpus: f64,
    #[serde(rename = "requestedMemoryMiB")]
    pub requested_memory_mib: f64,
    pub new_clients: u32,
    pub new_desktops: u32,
    pub queued_ahead: u32,
    pub exclusive_waiting: u32,
    pub queued_ahead_tickets: Vec<AheadTicket>,
    pub queued_ahead_new_clients: Vec<u32>,
    pub before_drain: bool,
    pub alone: bool,
    pub owner: Option<String>,
    pub pair: Option<u32>,
    pub beside_native: bool,
    pub quiet: bool,
    pub ticket: Option<String>,
    pub requester_lease: Option<String>,
    pub need: Option<String>,
    pub within_exclusive: bool,
    #[serde(default)]
    pub holds_offload_slot: bool,
}

/// The host part of the facts: signals, GPU, leases, queue, markers and what enrichment read.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub profile: Profile,
    pub cores: u32,
    pub slice_cpus: Option<f64>,
    #[serde(rename = "memoryTotalMiB")]
    pub memory_total_mib: f64,
    #[serde(rename = "memoryAvailableMiB")]
    pub memory_available_mib: f64,
    pub cpu_some_avg10_basis_points: u32,
    pub protected_cpu_some_avg10_basis_points: u32,
    pub cpu_pressure_measured: bool,
    #[serde(rename = "leasedMemoryMiB")]
    pub leased_memory_mib: f64,
    pub leased_batch_cpus: f64,
    pub unleased_heavy_cores: f64,
    pub leased_native_cpus: f64,
    pub peer_batch_runs: u32,
    pub peer_exclusive_runs: u32,
    pub unbounded_runs: u32,
    pub gpu_busy_percent: u32,
    pub gpu_clients: u32,
    pub host_warcraft_clients: Option<u32>,
    #[serde(rename = "bar1FreeMiB")]
    pub bar1_free_mib: Option<f64>,
    #[serde(rename = "bar1TimedOut")]
    pub bar1_timed_out: bool,
    #[serde(rename = "vramFreeMiB")]
    pub vram_free_mib: Option<f64>,
    #[serde(rename = "hostBar1MarginMiB")]
    pub host_bar1_margin_mib: Option<u32>,
    pub gpu_runs: u32,
    pub native_waiting: u32,
    pub land_check_waiting: bool,
    pub leases: Vec<Lease>,
    pub queue: Vec<QueueEntry>,
    pub exclusive_leases: Vec<String>,
    /// The Wisp LAN pair leases this host's queue directory holds.
    pub queued_pair_leases: Vec<PairLease>,
    pub gaming: bool,
    pub game_reserve_agent_cpus: Option<u32>,
    pub run_queue_load: Option<f64>,
    #[serde(rename = "diskFreeGiB")]
    pub disk_free_gib: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lease {
    pub id: String,
    pub class: String,
    pub owner: String,
    pub quiet: bool,
    pub pair: Option<u32>,
    pub created_at: f64,
    #[serde(rename = "memoryMiB")]
    pub memory_mib: f64,
    pub quiet_seconds: Option<f64>,
    pub usage: Option<UsageSample>,
    pub new_clients: u32,
    pub running_clients: u32,
    pub clients_seen: u32,
    pub started_owners: BTreeMap<String, u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSample {
    pub at: Option<f64>,
    pub last_busy_at: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueEntry {
    pub name: String,
    pub class: String,
    pub owner: Option<String>,
    pub reason: Option<String>,
    pub alone: bool,
    pub queued_at: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PairLease {
    pub pair: u32,
    pub owner: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AheadTicket {
    pub requested_class: Option<Class>,
    pub requested_cpus: Option<f64>,
    pub queued_seconds: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Profile {
    Attended,
    Unattended,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub reason: Reason,
    pub offload: Option<String>,
    pub bar1_need_mib: Option<f64>,
    pub disk_free_gib: Option<f64>,
    pub pair: Option<PairRefusal>,
    pub quiet_owners: Vec<String>,
    /// Whether the kernel reclaims disk and decides again, in a call that follows this one.
    pub reclaim: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Reason {
    Run,
    Invalid,
    DeferMemoryHeadroom,
    DeferMemoryCapacity,
    DeferNativeCpus,
    DeferGpuClients,
    DeferGpuBusy,
    DeferGpuAperture,
    DeferGpuApertureTimeout,
    DeferUnboundedPeer,
    DeferExclusive,
    DeferInteractivePressure,
    DeferNativeQuiet,
    DeferNativeWaiting,
    DeferGpuSlots,
    DeferQueued,
    DeferExclusiveQueued,
    DeferCpuCapacity,
    DeferCpuPressure,
    DeferBatchJobs,
    DeferGaming,
    DeferGameMemory,
    DeferRunQueue,
    DeferDiskFree,
    DeferNativeAlone,
    DeferLandCheckWaiting,
    PairHeld,
}

/// The pair that holds the request's pair, and the free pair it may take instead.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PairRefusal {
    pub pair: u32,
    pub holder: String,
    pub free_pair: u32,
}
