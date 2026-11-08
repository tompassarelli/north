use std::path::PathBuf;

use north_github_mirror::store::Mirror;

pub struct Places {
    pub mirror: Mirror,
    pub ledger: PathBuf,
    pub threads: PathBuf,
    pub pushes: PathBuf,
    pub code: PathBuf,
    pub corrections: PathBuf,
    pub ranking: PathBuf,
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
            mirror: Mirror::local(),
            ledger: north.join("ledger.db"),
            threads: north_threads::threads::client::threads_database(),
            pushes: north_fleet::fleet::landings::push_log(),
            code: PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code"),
            corrections: std::env::var_os("NORTH_GROUNDING_LEDGER").map_or_else(|| north.join("grounding/ledger.jsonl"), PathBuf::from),
            ranking: PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join("code/north/main/docs/rationale/backlog-ranking.md"),
            coverage: north.join("coverage"),
            land_reuse: north.join("land-reuse"),
            verdict_cache: north_base::verdict_cache::Store::here().0,
        }
    }
}
