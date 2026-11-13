use super::build_offload;

/// Ships a runtime this host holds to the host that lacks it: the command layer's runtime staging.
pub type Stage = fn(host: &str, sha: &str, runtime: &std::path::Path) -> Result<(), String>;
/// Runs `north` outside a worker's Landlock domain; None outside one.
pub type Reenter = fn(arguments: &[String]) -> Option<u8>;

#[derive(Clone, Copy)]
pub struct Host {
    pub serving: fn(&str) -> Option<String>,
    pub warcraft_clients: fn() -> Option<u32>,
    pub bar1_margin_mib: fn() -> Option<u32>,
    pub gpu_slots: fn() -> Option<u32>,
    pub offload: fn(&[String], Stage, Reenter) -> u8,
    /// Ships this host's runtime to a build host whose offload run finds it missing.
    pub stage_runtime: Stage,
    /// Runs a sandboxed worker's offload request through the outside broker.
    pub reenter_outside: Reenter,
    pub build_offload: build_offload::Offloader,
    pub hand_over: build_offload::HandOver,
    pub lease_builds: fn() -> bool,
    /// Whether this host is a build host other hosts send builds to (`[hosts.NAME] offload_builds`), so its own sessions' builds decide here first.
    pub build_host: fn() -> bool,
    /// Why this host has no room for a fleet session's build that no build host took, by the rule the capacity taker applies before it runs a pulled run here; None when it has room.
    pub own_build_refusal: fn() -> Option<String>,
    /// Whether this host may compile a fleet session's Rust build that no build host took (`[hosts.NAME] session_builds`, absent means it may).
    pub session_builds: fn() -> bool,
    pub reclaim_disk: fn() -> Option<f64>,
    /// Starts the disk pass as its own unit, which outlives the lease reaper's 25 s start timeout.
    pub tend_disk: fn(),
    /// Runs one disk pass here: `north capacity disk keep`.
    pub keep_disk: fn(),
    /// Warms one land-check slot toward its repository's head while no land check runs here.
    pub tend_slots: fn(),
}
