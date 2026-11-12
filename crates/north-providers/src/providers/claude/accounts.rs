use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};
use toml_edit::{DocumentMut, Item};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Account, ApiKey, Machine, ProviderName};
use crate::fleet::refusals;
use crate::fleet::roster::{Declared, Refused, Roster, Spent};
use crate::fleet::runtime::labelled_homes;

use super::api_pool::{self, Org};
use crate::credit::Grant;

const PREFIX: &str = "claude-";
const CREDENTIALS: &str = ".credentials.json";

#[derive(Debug, Default)]
pub struct Profiles {
    pub keepers: BTreeMap<String, String>,
    pub homes: Vec<(String, PathBuf)>,
    pub tokens: Vec<(String, PathBuf)>,
    pub api: Vec<(String, PathBuf)>,
    pub orgs: Vec<Org>,
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
    let declared_token = |label: &str| profiles.tokens.iter().find(|(token_label, _)| token_label == label).map(|(_, file)| file);
    let mut found: Vec<(Account, Option<String>)> = homes
        .into_iter()
        .filter(|(label, _)| declared_token(label).is_none_or(|file| readable_token(file)))
        .map(|(label, home)| {
            let identity = identity(&home, &profiles.default_home);
            let token_file = declared_token(&label).cloned();
            (Account { provider: provider.clone(), label, home, token_file, api: None }, identity)
        })
        .collect();
    let owned_elsewhere: Vec<(Account, Option<String>)> = profiles
        .elsewhere
        .keys()
        .filter(|label| !found.iter().any(|(account, _)| &&account.label == label))
        .map(|label| (Account { provider: provider.clone(), label: label.clone(), home: state_root.join("profiles").join(format!("{PREFIX}{label}")), token_file: None, api: None }, None))
        .collect();
    let tokens: Vec<(Account, Option<String>)> = profiles
        .tokens
        .iter()
        .filter(|(label, file)| !found.iter().any(|(account, _)| &account.label == label) && readable_token(file))
        .filter_map(|(label, file)| {
            let (_, home) = profiles.homes.iter().find(|(home_label, _)| home_label == label)?;
            Some((Account { provider: provider.clone(), label: label.clone(), home: home.clone(), token_file: Some(file.clone()), api: None }, None))
        })
        .collect();
    let (api, spent) = api_accounts(provider, &state_root.join("api-exhausted"), profiles, readable_token);
    found.extend(api);
    found.extend(tokens);
    found.extend(owned_elsewhere);
    let holders = profiles
        .homes
        .iter()
        .filter(|(label, _)| !found.iter().any(|(account, _)| &account.label == label))
        .filter_map(|(label, home)| Some((label.clone(), identity(home, &profiles.default_home)?)))
        .collect();
    let roster = Roster { spent, ..Roster::distinct(found, &Declared { keepers: profiles.keepers.clone(), holders, elsewhere: profiles.elsewhere.clone() }) };
    refused_aside(roster, &refusals::directory(state_root))
}

/// Moves each subscription account its client refused out of the accounts placement and launches may use, keeping the refusal text.
fn refused_aside(mut roster: Roster, marks: &Path) -> Roster {
    let (refused, usable): (Vec<Account>, Vec<Account>) = roster.accounts.into_iter().partition(|account| account.api.is_none() && refusals::held(marks, &account.label).is_some());
    roster.accounts = usable;
    roster.refused = refused.into_iter().filter_map(|account| Some(Refused { text: refusals::held(marks, &account.label)?, account })).collect();
    roster
}

/// The pooled API account `label` launches on the entry whose credit expires soonest, and one pinned account `label:ORG` per entry, so a session can be relaunched on the key it ran on.
fn api_accounts(provider: &ProviderName, marks: &Path, profiles: &Profiles, usable: impl Fn(&Path) -> bool + Copy) -> (Vec<(Account, Option<String>)>, Vec<Spent>) {
    let account = |label: String, home: &PathBuf, key: ApiKey| (Account { provider: provider.clone(), label, home: home.clone(), token_file: None, api: Some(key) }, None);
    let (mut accounts, mut spent) = (Vec::new(), Vec::new());
    for (label, home) in &profiles.api {
        let pinned = |org: &Org| org.keys.iter().find(|file| usable(file)).map(|file| ApiKey { org: org.name.clone(), file: file.clone() });
        if let Some((_, key)) = api_pool::open(&profiles.orgs, &api_pool::today(), |org| api_pool::exhausted(marks, org), usable) {
            accounts.push(account(label.clone(), home, key));
        } else if let Some(orgs) = api_pool::spent(&profiles.orgs, marks) {
            spent.push(Spent { provider: provider.clone(), label: label.clone(), orgs });
        }
        accounts.extend(profiles.orgs.iter().filter_map(|org| Some(account(format!("{label}:{}", org.name), home, pinned(org)?))));
    }
    (accounts, spent)
}

fn open_grants(policy: &Path, home: &Path) -> BTreeMap<String, Vec<Grant>> {
    crate::credit::open_grants(policy, &home.join(".local/state/north/ledger.db"), &api_pool::today())
}

pub fn declared(path: &Path, home: &Path) -> Profiles {
    let policy = std::fs::read_to_string(path).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    profiles(&policy, &open_grants(path, home), home, &crate::platform::host::name())
}

/// The account `label` on `machine`; an api account takes the key file `machine` declares, which this host cannot read.
pub fn on_machine(provider: &ProviderName, path: &Path, home: &Path, label: &str, machine: &str) -> NorthResult<Account> {
    let policy: DocumentMut = std::fs::read_to_string(path).ok().and_then(|text| text.parse().ok()).unwrap_or_default();
    let keyed = profiles(&policy, &open_grants(path, home), home, machine);
    let marks = home.join(".local/state/north/api-exhausted");
    let (api, spent) = api_accounts(provider, &marks, &keyed, |_: &Path| true);
    if let Some((account, _)) = api.into_iter().find(|(account, _)| account.label == label) {
        return Ok(account);
    }
    if let Some(spent) = spent.iter().find(|spent| spent.label == label) {
        return Err(NorthError::Refused(spent.to_string()));
    }
    let hosts = policy.get("accounts").and_then(|accounts| accounts.get("claude")).and_then(|claude| claude.get("profiles")).and_then(|profiles| profiles.get(label)).and_then(Item::as_table_like);
    let owners: Vec<&str> = hosts.into_iter().flat_map(|hosts| hosts.iter().map(|(host, _)| host)).collect();
    let entry = hosts.and_then(|hosts| hosts.get(machine)).ok_or_else(|| match owners.as_slice() {
        [] => NorthError::Usage(format!("no {} account named {label} is declared for {machine}", provider.label())),
        owners => NorthError::Refused(format!("{}:{label} is declared for {}, not {machine}; launch on {} or another --account", provider.label(), owners.join(" and "), owners.join(" or "))),
    })?;
    let root = session_root(&policy, machine, home).unwrap_or_else(|| home.to_path_buf());
    let path = |key: &str| entry.get(key).and_then(Item::as_str).map(|path| expand(&root, path));
    Ok(Account { provider: provider.clone(), label: label.to_owned(), home: path("path").unwrap_or_else(|| default_home(&root)), token_file: path("token_file"), api: None })
}

fn session_root(policy: &DocumentMut, machine: &str, home: &Path) -> Option<PathBuf> {
    let login = policy.get("hosts")?.get(machine)?.get("agent_ssh")?.as_str()?;
    Machine::login_home(login, home)
}

pub(super) fn default_home(home: &Path) -> PathBuf {
    home.join(".claude")
}

fn expand(home: &Path, path: &str) -> PathBuf {
    path.strip_prefix("~/").map_or_else(|| PathBuf::from(path), |rest| home.join(rest))
}

fn profiles(policy: &DocumentMut, grants: &BTreeMap<String, Vec<Grant>>, home: &Path, here: &str) -> Profiles {
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
    let api = table("api_keys").filter_map(|(label, hosts)| Some((label.to_owned(), expand(home, hosts.get(here)?.get("path")?.as_str()?)))).collect();
    Profiles { keepers, homes, tokens, api, orgs: api_pool::declared(policy, grants, home, here), elsewhere, default_home: default_home(home) }
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
