//! Where a build host's root disk counts as too full, as free space rather than a percent so a large disk keeps the
//! same headroom as a small one. The eviction line is the one placement and the lease reaper share: the reaper evicts
//! offload slots under it, and placement refuses a host under it, so a host is never refused while eviction sleeps.
use super::admission::default_mib as default_number;

const EVICT_FREE_SHARE: f64 = 0.20;
const INCIDENT_FREE_SHARE: f64 = 0.15;

/// Free space under the smaller of a share of the disk's size and a fixed headroom.
#[derive(Clone, Copy)]
pub struct Line {
    pub free_share: f64,
    pub free_gib: f64,
}

impl Line {
    /// `offload_evict_free_gib` free, or 20% of a disk too small to spare it.
    pub fn eviction() -> Self {
        Self { free_share: EVICT_FREE_SHARE, free_gib: default_number("offload_evict_free_gib") }
    }

    /// `host_disk_incident_free_gib` free, or 15% of a disk too small to spare it.
    pub fn incident() -> Self {
        Self { free_share: INCIDENT_FREE_SHARE, free_gib: default_number("host_disk_incident_free_gib") }
    }

    /// The free GiB under which a disk of `size_gib` passes this line; 0 for a disk of unknown size.
    pub fn floor_gib(self, size_gib: f64) -> f64 {
        (size_gib * self.free_share).min(self.free_gib)
    }

    pub fn passed(self, free_gib: f64, size_gib: f64) -> bool {
        free_gib < self.floor_gib(size_gib)
    }
}
