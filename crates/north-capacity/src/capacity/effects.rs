use super::build_offload;

#[derive(Clone, Copy)]
pub struct Host {
    pub serving: fn(&str) -> Option<String>,
    pub warcraft_clients: fn() -> Option<u32>,
    pub bar1_margin_mib: fn() -> Option<u32>,
    pub offload: fn(&[String]) -> u8,
    pub build_offload: build_offload::Offloader,
    pub hand_over: build_offload::HandOver,
    pub lease_builds: fn() -> bool,
    /// Why this host has no room for a fleet session's build that no build host took, by the rule the capacity taker applies before it runs a pulled run here; None when it has room.
    pub own_build_refusal: fn() -> Option<String>,
    pub reclaim_disk: fn() -> Option<f64>,
    pub tend_disk: fn(),
    /// Warms one land-check slot toward its repository's head while no land check runs here.
    pub tend_slots: fn(),
}
