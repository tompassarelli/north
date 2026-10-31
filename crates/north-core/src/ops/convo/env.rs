use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(super) const SCHEMA_VERSION: u32 = 4;

pub(super) struct Env {
    pub home: PathBuf,
    pub root: PathBuf,
    pub root_pinned: bool,
    pub state: PathBuf,
    pub zstd: String,
    pub roster_cmd: Option<String>,
    pub codex_home: Option<String>,
    pub pooled_home: Option<String>,
    pub claude_config: Option<String>,
    pub self_ids: Vec<String>,
    pub lock_wait: Duration,
}

fn set(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

pub(super) fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

impl Env {
    pub(super) fn from_env() -> Self {
        let home = PathBuf::from(env::var_os("HOME").unwrap_or_default());
        let pinned = set("CONVO_ROOT");
        let root = pinned.as_deref().map_or_else(|| home.join("code/north-data/accounts"), PathBuf::from);
        Self {
            root: canonical(&root),
            root_pinned: pinned.is_some(),
            state: set("CONVO_STATE").map_or_else(|| home.join(".local/state/convo"), PathBuf::from),
            zstd: set("CONVO_ZSTD").unwrap_or_else(|| "zstd".to_owned()),
            roster_cmd: set("CONVO_ROSTER_CMD"),
            codex_home: set("CODEX_HOME"),
            pooled_home: set("NORTH_CODEX_POOLED_HOME"),
            claude_config: set("CLAUDE_CONFIG_DIR"),
            self_ids: ["CLAUDE_CODE_SESSION_ID", "CODEX_THREAD_ID"].iter().filter_map(|name| set(name)).collect(),
            lock_wait: Duration::from_millis(1500),
            home,
        }
    }

    pub(super) fn db(&self) -> PathBuf {
        self.state.join(format!("index-v{SCHEMA_VERSION}.db"))
    }

    pub(super) fn lock(&self) -> PathBuf {
        let mut name = self.db().into_os_string();
        name.push(".lock");
        PathBuf::from(name)
    }

    pub(super) fn ensure_state(&self) -> io::Result<()> {
        fs::create_dir_all(&self.state)
    }
}
