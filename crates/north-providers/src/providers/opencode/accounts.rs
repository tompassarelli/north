use std::path::{Path, PathBuf};

use north_foundation::error::{NorthError, NorthResult};
use north_fleet_supervisor::fleet::model::{Account, Machine, ProviderName};
use north_fleet_supervisor::fleet::roster::{Declared, Roster};

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

/// The account `label` as `machine` holds it: a machine keeps the home layout of this one, so its key sits at the same path under the home of the login its sessions run as, and the key file is the machine's own to hold, which this host cannot read.
pub fn on_machine(provider: &ProviderName, home: &Path, label: &str, machine: &Machine) -> NorthResult<Account> {
    if label != LABEL {
        return Err(NorthError::Usage(format!("no {} account named {label} exists for {}; the one account is {LABEL}", provider.label(), machine.name)));
    }
    let root = Machine::login_home(machine.session_login(), home).unwrap_or_else(|| home.to_path_buf());
    let state_root = root.join(".local/state/north");
    Ok(Account { provider: provider.clone(), label: LABEL.into(), token_file: Some(key_file(&state_root)), home: state_root.join("accounts/opencode"), api: None })
}
