use std::path::{Path, PathBuf};

use crate::fleet::model::{Account, ProviderName};
use crate::fleet::roster::{Declared, Roster};
use crate::fleet::runtime::labelled_homes;

const PREFIX: &str = "codex-";
const POOLED: &str = "pooled";

pub fn discover(provider: &ProviderName, state_root: &Path, home: &Path) -> Roster {
    let default_home = pooled_home(home);
    let default = default_home
        .join("auth.json")
        .is_file()
        .then(|| Account { provider: provider.clone(), label: POOLED.into(), home: default_home.clone() });
    let named = labelled_homes(&state_root.join("accounts/openai"), PREFIX, "auth.json")
        .into_iter()
        .map(|(label, home)| Account { provider: provider.clone(), label, home });
    Roster::distinct(default.into_iter().chain(named).map(|account| (account, None)), &Declared::default())
}

pub fn pooled_home(home: &Path) -> PathBuf {
    ["NORTH_CODEX_CONVERSATION_HOME", "NORTH_CODEX_POOLED_HOME"]
        .iter()
        .filter_map(std::env::var_os)
        .find(|value| !value.is_empty())
        .map_or_else(|| home.join(".local/state/north/codex-pooled"), PathBuf::from)
}
