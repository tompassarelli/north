use std::path::Path;

use crate::fleet::model::{Account, ProviderName};
use crate::fleet::runtime::labelled_homes;

const PREFIX: &str = "claude-";

pub fn discover(provider: &ProviderName, state_root: &Path) -> Vec<Account> {
    labelled_homes(&state_root.join("profiles"), PREFIX, ".credentials.json")
        .into_iter()
        .map(|(label, home)| Account { provider: provider.clone(), label, home })
        .collect()
}
