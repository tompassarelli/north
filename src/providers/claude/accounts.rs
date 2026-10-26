use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, Item};

use crate::fleet::model::{Account, ProviderName};
use crate::fleet::roster::{Declared, Roster};
use crate::fleet::runtime::labelled_homes;

const PREFIX: &str = "claude-";
const CREDENTIALS: &str = ".credentials.json";

#[derive(Debug, Default)]
pub struct Profiles {
    pub keepers: BTreeMap<String, String>,
    pub homes: Vec<(String, PathBuf)>,
    pub elsewhere: BTreeMap<String, Vec<String>>,
    pub default_home: PathBuf,
}

pub fn discover(provider: &ProviderName, state_root: &Path, profiles: &Profiles) -> Roster {
    let mut homes: Vec<(String, PathBuf)> = profiles.homes.iter().filter(|(_, home)| home.join(CREDENTIALS).is_file()).cloned().collect();
    for (label, home) in labelled_homes(&state_root.join("profiles"), PREFIX, CREDENTIALS) {
        if !homes.iter().any(|(declared, _)| declared == &label) {
            homes.push((label, home));
        }
    }
    homes.sort();
    let mut found: Vec<(Account, Option<String>)> = homes
        .into_iter()
        .map(|(label, home)| {
            let identity = identity(&home, &profiles.default_home);
            (Account { provider: provider.clone(), label, home }, identity)
        })
        .collect();
    let owned_elsewhere: Vec<(Account, Option<String>)> = profiles
        .elsewhere
        .keys()
        .filter(|label| !found.iter().any(|(account, _)| &&account.label == label))
        .map(|label| (Account { provider: provider.clone(), label: label.clone(), home: state_root.join("profiles").join(format!("{PREFIX}{label}")) }, None))
        .collect();
    found.extend(owned_elsewhere);
    let holders = profiles
        .homes
        .iter()
        .filter(|(label, _)| !found.iter().any(|(account, _)| &account.label == label))
        .filter_map(|(label, home)| Some((label.clone(), identity(home, &profiles.default_home)?)))
        .collect();
    Roster::distinct(found, &Declared { keepers: profiles.keepers.clone(), holders, elsewhere: profiles.elsewhere.clone() })
}

pub fn declared(policy: &Path, home: &Path) -> Profiles {
    let policy = std::fs::read_to_string(policy).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    profiles(&policy, home, &crate::platform::host::name())
}

pub(super) fn default_home(home: &Path) -> PathBuf {
    home.join(".claude")
}

fn profiles(policy: &DocumentMut, home: &Path, here: &str) -> Profiles {
    let claude = policy.get("accounts").and_then(|accounts| accounts.get("claude"));
    let table = |key: &str| claude.and_then(|claude| claude.get(key)).and_then(Item::as_table_like).into_iter().flat_map(|table| table.iter());
    let keepers = table("labels").filter_map(|(identity, label)| Some((identity.to_owned(), label.as_str()?.to_owned()))).collect();
    let homes = table("profiles")
        .filter_map(|(label, hosts)| {
            let path = hosts.get(here)?.get("path")?.as_str()?;
            Some((label.to_owned(), path.strip_prefix("~/").map_or_else(|| PathBuf::from(path), |rest| home.join(rest))))
        })
        .collect();
    let elsewhere = table("profiles")
        .filter_map(|(label, hosts)| {
            let owners: Vec<String> = hosts.as_table_like()?.iter().map(|(host, _)| host.to_owned()).collect();
            (!owners.is_empty() && !owners.iter().any(|owner| owner == here)).then(|| (label.to_owned(), owners))
        })
        .collect();
    Profiles { keepers, homes, elsewhere, default_home: default_home(home) }
}

pub(super) fn config_file(home: &Path, default: &Path) -> PathBuf {
    if home == default { home.with_file_name(".claude.json") } else { home.join(".claude.json") }
}

fn identity(home: &Path, default: &Path) -> Option<String> {
    let config: Value = serde_json::from_str(&std::fs::read_to_string(config_file(home, default)).ok()?).ok()?;
    let uuid = config["oauthAccount"]["accountUuid"].as_str().filter(|uuid| !uuid.is_empty())?;
    Some(format!("{:x}", Sha256::digest(uuid.as_bytes()))[..8].to_owned())
}

#[cfg(test)]
#[path = "accounts_tests.rs"]
mod tests;
