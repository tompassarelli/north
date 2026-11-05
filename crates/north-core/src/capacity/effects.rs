use super::build_offload;

#[derive(Clone, Copy)]
pub struct Host {
    pub serving: fn(&str) -> Option<String>,
    pub warcraft_clients: fn() -> Option<u32>,
    pub bar1_margin_mib: fn() -> Option<u32>,
    pub offload: fn(&[String]) -> u8,
    pub build_offload: build_offload::Offloader,
    pub lease_builds: fn() -> bool,
    pub reclaim_disk: fn() -> Option<f64>,
}
