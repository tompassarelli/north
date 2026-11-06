use std::path::PathBuf;

use north_github_mirror::store::Mirror;

pub struct Places {
    pub mirror: Mirror,
    pub ledger: PathBuf,
    pub pushes: PathBuf,
    pub code: PathBuf,
    pub decisions: Option<PathBuf>,
    pub corrections: PathBuf,
    pub coverage: PathBuf,
}

fn state() -> PathBuf {
    let home = || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state");
    std::env::var_os("XDG_STATE_HOME").filter(|dir| !dir.is_empty()).map_or_else(home, PathBuf::from)
}

impl Places {
    pub fn local() -> Self {
        let north = state().join("north");
        Self {
            mirror: Mirror::local(),
            ledger: north.join("ledger.db"),
            pushes: north_fleet::fleet::landings::push_log(),
            code: PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code"),
            decisions: north_base::platform::decision_log::root(),
            corrections: std::env::var_os("NORTH_GROUNDING_LEDGER").map_or_else(|| north.join("grounding/ledger.jsonl"), PathBuf::from),
            coverage: north.join("coverage"),
        }
    }
}
