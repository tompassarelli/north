use super::super::sampling;
use super::local;
use crate::fleet::model::ProviderName;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use toml_edit::Item;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Login {
    pub label: String,
    pub home: PathBuf,
    pub identity: Option<String>,
    pub shares: Option<String>,
}

impl Login {
    pub(crate) fn refusal(&self) -> Option<String> {
        self.shares.as_ref().map(|keeper| format!("claude:{} is logged in to the same Claude account as claude:{keeper}", self.label))
    }
}

pub(crate) fn identity(home: &Path) -> Option<String> {
    let text = std::fs::read_to_string(home.join(".claude.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let uuid = value["oauthAccount"]["accountUuid"].as_str()?;
    Some(format!("{:x}", Sha256::digest(uuid.as_bytes()))[..8].to_owned())
}

pub(crate) fn logins(homes: Vec<(String, PathBuf)>, identity: impl Fn(&Path) -> Option<String>, declared: &BTreeMap<String, String>) -> Vec<Login> {
    let mut logins: Vec<Login> = Vec::new();
    for (label, home) in homes {
        if !logins.iter().any(|login| login.label == label) {
            logins.push(Login { identity: identity(&home), label, home, shares: None });
        }
    }
    logins.sort_by(|a, b| a.label.cmp(&b.label));
    let shares: Vec<Option<String>> = logins
        .iter()
        .map(|login| {
            let id = login.identity.as_ref()?;
            let sharing: Vec<&String> = logins.iter().filter(|other| other.identity.as_ref() == Some(id)).map(|other| &other.label).collect();
            let keeper = declared.get(id).filter(|label| sharing.contains(label)).unwrap_or(sharing[0]);
            (keeper != &login.label).then(|| keeper.clone())
        })
        .collect();
    for (login, shares) in logins.iter_mut().zip(shares) {
        login.shares = shares;
    }
    logins
}

pub(crate) fn live() -> Vec<Login> {
    let cfg = sampling::config().ok();
    let claude = cfg.as_ref().and_then(|cfg| cfg.get("accounts")).and_then(|accounts| accounts.get("claude"));
    let declared: BTreeMap<String, String> = claude
        .and_then(|claude| claude.get("labels"))
        .and_then(Item::as_table_like)
        .into_iter()
        .flat_map(|labels| labels.iter())
        .filter_map(|(id, label)| Some((id.to_owned(), label.as_str()?.to_owned())))
        .collect();
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let here = local();
    let declared_homes = claude
        .and_then(|claude| claude.get("profiles"))
        .and_then(Item::as_table_like)
        .into_iter()
        .flat_map(|profiles| profiles.iter())
        .filter_map(|(label, hosts)| {
            let path = hosts.get(&here)?.get("path")?.as_str()?;
            Some((label.to_owned(), path.strip_prefix("~/").map_or_else(|| PathBuf::from(path), |rest| home.join(rest))))
        });
    let fleet = crate::providers::claude::discover(&ProviderName::new("claude"), &crate::fleet::registry::state_dir(&home));
    let homes = fleet.into_iter().map(|account| (account.label, account.home)).chain(declared_homes).collect();
    logins(homes, identity, &declared)
}

pub(crate) fn shared(logins: &[Login]) -> BTreeMap<String, String> {
    logins
        .iter()
        .filter_map(|login| Some((format!("claude:{}", login.label), format!("claude:{}", login.shares.as_ref()?))))
        .collect()
}
