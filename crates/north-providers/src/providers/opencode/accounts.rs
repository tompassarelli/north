use std::path::{Path, PathBuf};

use crate::fleet::model::{Account, ProviderName};
use crate::fleet::roster::{Declared, Roster};

pub const LABEL: &str = "go";
const KEY_FILE: &str = "go.key";

pub fn key_file(state_root: &Path) -> PathBuf {
    state_root.join("accounts/opencode").join(KEY_FILE)
}

/// The one account an opencode worker runs on: the file under the state root that holds the paid Go key, which the free model never needs.
pub fn discover(provider: &ProviderName, state_root: &Path) -> Roster {
    let home = state_root.join("accounts/opencode");
    let key = key_file(state_root);
    let account = Account { provider: provider.clone(), label: LABEL.into(), token_file: key.is_file().then_some(key), home, api: None };
    Roster::distinct([(account, None)], &Declared::default())
}
