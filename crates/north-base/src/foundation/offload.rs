use std::env;
use std::path::PathBuf;

pub const ROOT_ENV: &str = "NORTH_OFFLOAD_ROOT";
pub const ROOT_DEFAULT: &str = ".cache/land-offload";
pub const OWNER_ENV: &str = "NORTH_OFFLOAD_OWNER";
pub const RED_FIX_ENV: &str = "NORTH_LAND_RED_FIX";
pub const LANDER_SLOT_ENV: &str = "NORTH_OFFLOAD_LANDER_SLOT";
pub const SLOT_ENV: &str = "NORTH_OFFLOAD_SLOT";
pub const LOCAL_ENV: &str = "NORTH_LAND_CHECKS_LOCAL";
pub const OFF_HOST_ENV: &str = "NORTH_LAND_CHECKS_OFF_HOST";

pub fn root() -> Option<PathBuf> {
    env::var_os(ROOT_ENV)
        .filter(|root| !root.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").filter(|home| !home.is_empty()).map(|home| PathBuf::from(home).join(ROOT_DEFAULT)))
}
