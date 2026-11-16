//! The caller's home, state and configuration directories, by the XDG rules north's runtime follows.

use std::env;
use std::path::PathBuf;

pub struct Paths {
    pub state_home: PathBuf,
    pub config_home: PathBuf,
}

impl Paths {
    pub fn here() -> Result<Self, String> {
        let home = env::var_os("HOME").filter(|home| !home.is_empty()).map(PathBuf::from).ok_or("HOME is unset")?;
        let xdg = |name: &str, fallback: &str| env::var_os(name).map(PathBuf::from).filter(|path| path.is_absolute()).unwrap_or_else(|| home.join(fallback));
        Ok(Self { state_home: xdg("XDG_STATE_HOME", ".local/state"), config_home: xdg("XDG_CONFIG_HOME", ".config") })
    }
}
