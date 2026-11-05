use std::env;
use std::path::PathBuf;
use std::time::Duration;

pub fn state_dir() -> PathBuf {
    match env::var_os("NORTH_GROUNDING_STATE") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".local/state/north/grounding"),
    }
}

pub const SQLITE_BUSY: Duration = Duration::from_secs(30);
