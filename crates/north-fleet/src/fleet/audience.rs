use std::path::PathBuf;

pub fn fleet_dir() -> PathBuf {
    std::env::var_os("NORTH_FLEET_DIR").map_or_else(|| std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default().join(".local/state/north/fleet"), PathBuf::from)
}
