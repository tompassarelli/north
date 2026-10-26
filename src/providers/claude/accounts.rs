use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, Item};

use crate::fleet::model::{Account, ProviderName};
use crate::fleet::roster::{Declared, Roster};
use crate::fleet::runtime::labelled_homes;

const PREFIX: &str = "claude-";

#[derive(Debug, Default)]
pub struct Profiles {
    pub keepers: BTreeMap<String, String>,
    pub homes: Vec<(String, PathBuf)>,
    pub elsewhere: BTreeMap<String, Vec<String>>,
}

pub fn discover(provider: &ProviderName, state_root: &Path, profiles: &Profiles) -> Roster {
    let mut found: Vec<(Account, Option<String>)> = labelled_homes(&state_root.join("profiles"), PREFIX, ".credentials.json")
        .into_iter()
        .map(|(label, home)| {
            let identity = identity(&home);
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
        .filter_map(|(label, home)| Some((label.clone(), identity(home)?)))
        .collect();
    Roster::distinct(found, &Declared { keepers: profiles.keepers.clone(), holders, elsewhere: profiles.elsewhere.clone() })
}

pub fn declared(policy: &Path, home: &Path) -> Profiles {
    let policy = std::fs::read_to_string(policy).ok().and_then(|text| text.parse().ok());
    policy.map(|policy| profiles(&policy, home, &crate::platform::host::name())).unwrap_or_default()
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
    Profiles { keepers, homes, elsewhere }
}

fn identity(home: &Path) -> Option<String> {
    let config: Value = serde_json::from_str(&std::fs::read_to_string(home.join(".claude.json")).ok()?).ok()?;
    let uuid = config["oauthAccount"]["accountUuid"].as_str().filter(|uuid| !uuid.is_empty())?;
    Some(format!("{:x}", Sha256::digest(uuid.as_bytes()))[..8].to_owned())
}

#[cfg(test)]
#[path = "accounts_tests.rs"]
mod tests;
