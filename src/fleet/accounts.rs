use std::fs;
use std::path::{Path, PathBuf};

use super::model::{Account, Provider};

const CLAUDE_PREFIX: &str = "claude-";
const CODEX_PREFIX: &str = "codex-";
pub const POOLED_CODEX: &str = "pooled";

pub fn discover(state_root: &Path, home: &Path) -> Vec<Account> {
    let mut accounts = claude_accounts(&state_root.join("profiles"));
    accounts.extend(codex_accounts(&state_root.join("accounts/openai"), &pooled_codex_home(home)));
    accounts
}

fn claude_accounts(profiles: &Path) -> Vec<Account> {
    labelled(profiles, CLAUDE_PREFIX, ".credentials.json")
        .into_iter()
        .map(|(label, home)| Account { provider: Provider::Claude, label, home })
        .collect()
}

fn codex_accounts(openai: &Path, default_home: &Path) -> Vec<Account> {
    let default = default_home
        .join("auth.json")
        .is_file()
        .then(|| Account { provider: Provider::Codex, label: POOLED_CODEX.into(), home: default_home.to_path_buf() });
    let named = labelled(openai, CODEX_PREFIX, "auth.json")
        .into_iter()
        .map(|(label, home)| Account { provider: Provider::Codex, label, home });
    default.into_iter().chain(named).collect()
}

fn labelled(directory: &Path, prefix: &str, marker: &str) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(String, PathBuf)> = fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let label = name.strip_prefix(prefix)?.to_owned();
            entry.path().join(marker).is_file().then(|| (label, entry.path()))
        })
        .collect();
    found.sort();
    found
}

fn pooled_codex_home(home: &Path) -> PathBuf {
    ["NORTH_CODEX_CONVERSATION_HOME", "NORTH_CODEX_POOLED_HOME"]
        .iter()
        .filter_map(std::env::var_os)
        .find(|value| !value.is_empty())
        .map_or_else(|| home.join(".local/state/north/codex-pooled"), PathBuf::from)
}
