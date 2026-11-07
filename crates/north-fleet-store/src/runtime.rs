use std::path::PathBuf;

use crate::foundation::clock::Timestamp;

pub const PROXY: &str = "proxy";

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
