use std::path::Path;

use crate::fleet::model::{Account, ProviderName};
use crate::fleet::roster::{Declared, Roster};

pub const LABEL: &str = "go";
const KEY_FILE: &str = "go.key";

/// The one account an opencode worker runs on: the file under the state root that holds the paid Go key, which the free model never needs.
pub fn discover(provider: &ProviderName, state_root: &Path) -> Roster {
    let home = state_root.join("accounts/opencode");
    let key = home.join(KEY_FILE);
    let account = Account { provider: provider.clone(), label: LABEL.into(), token_file: key.is_file().then_some(key), home, api: None };
    Roster::distinct([(account, None)], &Declared::default())
}
