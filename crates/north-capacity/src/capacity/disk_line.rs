//! Where a build host's root disk counts as too full, as free space rather than a percent so a large disk keeps the
//! same headroom as a small one. Passing the high watermark starts one cleanup pass that trims regenerable caches least
//! recently used first down to the low watermark, far roomier, so one pass lasts weeks rather than trimming just back over
//! one line; under the eviction line the lease reaper also takes land-offload slots and placement refuses the host.
use super::admission::default_mib as default_number;

const LOW_WATERMARK_FREE_SHARE: f64 = 0.40;
const HIGH_WATERMARK_FREE_SHARE: f64 = 0.13;
const EVICT_FREE_SHARE: f64 = 0.20;

/// Free space under the smaller of a share of the disk's size and a fixed headroom.
#[derive(Clone, Copy)]
pub struct Line {
    pub free_share: f64,
    pub free_gib: f64,
}

impl Line {
    /// A line no disk passes.
    pub const NEVER: Self = Self { free_share: 0.0, free_gib: 0.0 };

    /// `disk_low_watermark_free_gib` free, or 40% of a disk too small to spare it.
    pub fn low_watermark() -> Self {
        Self { free_share: LOW_WATERMARK_FREE_SHARE, free_gib: default_number("disk_low_watermark_free_gib") }
    }

    /// `disk_high_watermark_free_gib` free, or 13% of a disk too small to spare it.
    pub fn high_watermark() -> Self {
        Self { free_share: HIGH_WATERMARK_FREE_SHARE, free_gib: default_number("disk_high_watermark_free_gib") }
    }

    /// `offload_evict_free_gib` free, or 20% of a disk too small to spare it.
    pub fn eviction() -> Self {
        Self { free_share: EVICT_FREE_SHARE, free_gib: default_number("offload_evict_free_gib") }
    }

    /// The free GiB under which a disk of `size_gib` passes this line; 0 for a disk of unknown size.
    pub fn floor_gib(self, size_gib: f64) -> f64 {
        (size_gib * self.free_share).min(self.free_gib)
    }

    pub fn passed(self, free_gib: f64, size_gib: f64) -> bool {
        free_gib < self.floor_gib(size_gib)
    }
}
