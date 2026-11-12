//! Where a build host's root disk counts as too full, as free space rather than a percent so a large disk keeps the
//! same headroom as a small one. A host closes to new work under its closing line, and the lease reaper holds the disk over
//! one rule above it, the closing line plus one warm slot of margin: every pass under that line trims regenerable caches least
//! recently used first toward the low watermark, far roomier, and takes warm stores until the disk is back over it, so a host
//! is closed only once nothing its rules may reclaim is left.
use super::admission::default_mib as default_number;

const LOW_WATERMARK_FREE_SHARE: f64 = 0.40;
const EVICT_FREE_SHARE: f64 = 0.20;
const EVICT_STOP_FREE_SHARE: f64 = 0.25;

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

    /// `offload_evict_free_gib` free, or 20% of a disk too small to spare it.
    pub fn eviction() -> Self {
        Self { free_share: EVICT_FREE_SHARE, free_gib: default_number("offload_evict_free_gib") }
    }

    /// `offload_evict_stop_free_gib` free, or 25% of a disk too small to spare it: its margin over the eviction line is the margin the held line keeps over the closing line.
    pub fn eviction_stop() -> Self {
        Self { free_share: EVICT_STOP_FREE_SHARE, free_gib: default_number("offload_evict_stop_free_gib") }
    }

    /// The line dispatch closes a host of `size_gib` under: its `disk_free_gib` dispatch bound `bound_gib` when that lies over the eviction line, else the eviction line.
    pub fn closed(bound_gib: Option<f64>, size_gib: f64) -> Self {
        bound_gib.filter(|bound| *bound > Self::eviction().floor_gib(size_gib)).map_or_else(Self::eviction, |bound| Self { free_share: 1.0, free_gib: bound })
    }

    /// The line the lease reaper holds a host of `size_gib` over: the closing line plus the margin the eviction stop line keeps over the eviction line, so the
    /// reaper reclaims while the host is still open rather than once it is closed.
    pub fn held(bound_gib: Option<f64>, size_gib: f64) -> Self {
        let margin = Self::eviction_stop().floor_gib(size_gib) - Self::eviction().floor_gib(size_gib);
        bound_gib.filter(|bound| *bound > Self::eviction().floor_gib(size_gib)).map_or_else(Self::eviction_stop, |bound| Self { free_share: 1.0, free_gib: bound + margin })
    }

    /// The free GiB under which a disk of `size_gib` passes this line; 0 for a disk of unknown size.
    pub fn floor_gib(self, size_gib: f64) -> f64 {
        (size_gib * self.free_share).min(self.free_gib)
    }

    pub fn passed(self, free_gib: f64, size_gib: f64) -> bool {
        free_gib < self.floor_gib(size_gib)
    }
}
