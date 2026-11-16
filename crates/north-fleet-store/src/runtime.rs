use std::path::PathBuf;

use north_foundation::foundation::clock::Timestamp;

pub use north_base::platform::session_scope::{COORDINATOR_ROLE as COORDINATOR, ROLE_ENV};
pub use north_warm_spares::names::{BINDING_ENV, Binding, SESSION_ENV, binding};
pub const SPOOLED_ENV: &str = "NORTH_FLEET_SPOOLED";

pub fn binary(name: &str) -> PathBuf {
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>())
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file());
    let local = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/bin").join(name));
    on_path.or(local).unwrap_or_else(|| PathBuf::from(name))
}

pub fn now_ms() -> u64 {
    u64::try_from(Timestamp::now().unix_millis()).unwrap_or(0)
}

