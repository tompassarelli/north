use std::path::PathBuf;

pub struct Places {
    pub code: PathBuf,
    pub coverage: PathBuf,
    pub land_reuse: PathBuf,
    pub verdict_cache: PathBuf,
}

fn state() -> PathBuf {
    let home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state");
    std::env::var_os("XDG_STATE_HOME").filter(|dir| !dir.is_empty()).map_or_else(home, PathBuf::from)
}

impl Places {
    pub fn local() -> Self {
        let north = state().join("north");
        Self {
            code: PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code"),
            coverage: north.join("coverage"),
            land_reuse: north.join("land-reuse"),
            verdict_cache: north_base::verdict_cache::Store::here().0,
        }
    }
}
