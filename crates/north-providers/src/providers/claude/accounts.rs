use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, Item};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Account, ProviderName};
use crate::fleet::roster::{Declared, Roster};
use crate::fleet::runtime::labelled_homes;

const PREFIX: &str = "claude-";
const CREDENTIALS: &str = ".credentials.json";

#[derive(Debug, Default)]
pub struct Profiles {
    pub keepers: BTreeMap<String, String>,
    pub homes: Vec<(String, PathBuf)>,
    pub tokens: Vec<(String, PathBuf)>,
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
            (Account { provider: provider.clone(), label, home, token_file: None }, identity)
        })
        .collect();
    let owned_elsewhere: Vec<(Account, Option<String>)> = profiles
        .elsewhere
        .keys()
        .filter(|label| !found.iter().any(|(account, _)| &&account.label == label))
        .map(|label| (Account { provider: provider.clone(), label: label.clone(), home: state_root.join("profiles").join(format!("{PREFIX}{label}")), token_file: None }, None))
        .collect();
    let tokens: Vec<(Account, Option<String>)> = profiles
        .tokens
        .iter()
        .filter(|(label, file)| !found.iter().any(|(account, _)| &account.label == label) && readable_token(file))
        .filter_map(|(label, file)| {
            let (_, home) = profiles.homes.iter().find(|(home_label, _)| home_label == label)?;
            Some((Account { provider: provider.clone(), label: label.clone(), home: home.clone(), token_file: Some(file.clone()) }, None))
        })
        .collect();
    found.extend(tokens);
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

pub fn on_machine(provider: &ProviderName, policy: &Path, home: &Path, label: &str, machine: &str) -> NorthResult<Account> {
    let policy: DocumentMut = std::fs::read_to_string(policy).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    let hosts = policy.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("profiles")).and_then(|profiles| profiles.get(label)).and_then(Item::as_table_like);
    let owners: Vec<&str> = hosts.into_iter().flat_map(|hosts| hosts.iter().map(|(host, _)| host)).collect();
    let entry = hosts.and_then(|hosts| hosts.get(machine)).ok_or_else(|| match owners.as_slice() {
        [] => NorthError::Usage(format!("no {} account named {label} is declared for {machine}", provider.label())),
        owners => NorthError::Refused(format!("{}:{label} is declared for {}, not {machine}; launch on {} or another --account", provider.label(), owners.join(" and "), owners.join(" or "))),
    })?;
    let path = |key: &str| entry.get(key).and_then(Item::as_str).map(|path| expand(home, path));
    Ok(Account { provider: provider.clone(), label: label.to_owned(), home: path("path").unwrap_or_else(|| default_home(home)), token_file: path("token_file") })
}

pub(super) fn default_home(home: &Path) -> PathBuf {
    home.join(".claude")
}

fn expand(home: &Path, path: &str) -> PathBuf {
    path.strip_prefix("~/").map_or_else(|| PathBuf::from(path), |rest| home.join(rest))
}

fn profiles(policy: &DocumentMut, home: &Path, here: &str) -> Profiles {
    let claude = policy.get("accounts").and_then(|accounts| accounts.get("claude"));
    let table = |key: &str| claude.and_then(|claude| claude.get(key)).and_then(Item::as_table_like).into_iter().flat_map(|table| table.iter());
    let keepers = table("labels").filter_map(|(identity, label)| Some((identity.to_owned(), label.as_str()?.to_owned()))).collect();
    let paths = |key: &str| -> Vec<(String, PathBuf)> {
        table("profiles")
            .filter_map(move |(label, hosts)| {
                let path = hosts.get(here)?.get(key)?.as_str()?;
                Some((label.to_owned(), expand(home, path)))
            })
            .collect()
    };
    let (homes, tokens) = (paths("path"), paths("token_file"));
    let elsewhere = table("profiles")
        .filter_map(|(label, hosts)| {
            let owners: Vec<String> = hosts.as_table_like()?.iter().map(|(host, _)| host.to_owned()).collect();
            (!owners.is_empty() && !owners.iter().any(|owner| owner == here)).then(|| (label.to_owned(), owners))
        })
        .collect();
    Profiles { keepers, homes, tokens, elsewhere, default_home: default_home(home) }
}

pub(super) fn readable_token(file: &Path) -> bool {
    std::fs::metadata(file).is_ok_and(|meta| meta.is_file() && meta.len() > 0) && std::fs::File::open(file).is_ok()
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
