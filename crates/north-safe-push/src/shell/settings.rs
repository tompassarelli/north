use std::path::PathBuf;

pub struct Settings {
    pub state_home: PathBuf,
    pub south: PathBuf,
    pub safe_push: String,
    pub fleet_session: String,
    pub raw_exit: bool,
}

impl Settings {
    pub fn from_env() -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        Self {
            state_home: std::env::var_os("XDG_STATE_HOME").map_or_else(|| home.join(".local/state"), PathBuf::from),
            south: std::env::var_os("SAFE_PUSH_SOUTH").map_or_else(|| home.join("code/south"), PathBuf::from),
            safe_push: std::env::var("SAFE_PUSH_SELF").unwrap_or_default(),
            fleet_session: std::env::var("NORTH_FLEET_SESSION").unwrap_or_default(),
            raw_exit: std::env::var_os("NORTH_NO_AUTOREAP").is_some_and(|value| value == "1"),
        }
    }
}
