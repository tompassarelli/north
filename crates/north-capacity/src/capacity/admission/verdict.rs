//! What admission returns: the reason, the offload host or aperture that explains a defer, and the pair
//! refusal and quiet owners its caller reports.

use serde::Serialize;

use super::super::native_hold::PairRefusal;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Admission {
    pub reason: Reason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offload: Option<String>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub aperture: Option<Aperture>,
    #[serde(rename = "diskFreeGiB", skip_serializing_if = "Option::is_none")]
    pub disk_free_gib: Option<f64>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub pair: Option<PairRefusal>,
    #[serde(rename = "quietOwners", skip_serializing_if = "Vec::is_empty")]
    pub quiet_owners: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Aperture {
    #[serde(rename = "bar1FreeMiB")]
    pub bar1_free_mib: f64,
    #[serde(rename = "bar1NeedMiB")]
    pub bar1_need_mib: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
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
    DeferGpuMemory,
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
