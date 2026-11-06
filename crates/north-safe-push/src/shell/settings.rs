use std::path::PathBuf;

use super::origin::repository;

const WATCH_DEFAULT: &str = ".local/state/north/current/north";

pub struct Settings {
    pub state_home: PathBuf,
    pub south: PathBuf,
    pub mirrors: PathBuf,
    pub safe_push: String,
    pub fleet_session: String,
    pub host: String,
    pub pid: u32,
    pub output_claimed: String,
    pub working_directory: PathBuf,
    pub watcher: Option<PathBuf>,
    pub raw_exit: bool,
    pub land_offload: bool,
}

impl Settings {
    pub fn from_env() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let watch = std::env::var_os("SAFE_PUSH_WATCH").filter(|value| !value.is_empty()).map(PathBuf::from);
        let real_north = std::env::var_os("SAFE_PUSH_TEST_REAL_NORTH").is_some_and(|value| !value.is_empty());
        Self {
            state_home: std::env::var_os("XDG_STATE_HOME").map_or_else(|| home.join(".local/state"), PathBuf::from),
            south: std::env::var_os("SAFE_PUSH_SOUTH").map_or_else(|| home.join("code/south"), PathBuf::from),
            mirrors: std::env::var_os("LANE_GC_MIRRORS").filter(|value| !value.is_empty()).map_or_else(|| home.join(".local/state/agents/lane-gc/mirrors"), PathBuf::from),
            safe_push: entry(),
            fleet_session: std::env::var("NORTH_FLEET_SESSION").unwrap_or_default(),
            host: std::fs::read_to_string("/proc/sys/kernel/hostname").map(|name| name.trim().to_owned()).unwrap_or_default(),
            pid: host_pid(),
            output_claimed: std::env::var("SAFE_PUSH_OUTPUT_CLAIMED").unwrap_or_default(),
            working_directory: std::env::current_dir().unwrap_or_default(),
            watcher: if real_north { watch } else { watch.or_else(|| Some(home.join(WATCH_DEFAULT))) },
            raw_exit: std::env::var_os("NORTH_NO_AUTOREAP").is_some_and(|value| value == "1"),
            land_offload: std::env::var_os("SAFE_PUSH_LAND_OFFLOAD").is_none_or(|value| value.is_empty() || value == "1"),
        }
    }

    pub fn events_file(&self) -> PathBuf {
        self.state_home.join("safe-push/events.jsonl")
    }

    pub fn mirror(&self, origin_url: &str) -> PathBuf {
        self.mirrors.join(format!("{}.git", repository(origin_url)))
    }
}

fn entry() -> String {
    std::env::current_exe().map(|path| path.to_string_lossy().into_owned()).unwrap_or_default()
}

fn host_pid() -> u32 {
    std::fs::read_to_string("/proc/self/stat").ok().and_then(|stat| stat.split_whitespace().next()?.parse().ok()).unwrap_or_else(std::process::id)
}
