use std::collections::BTreeMap;
use std::fmt;

use crate::error::{NorthError, NorthResult};

use super::model::{Account, ProviderName};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Roster {
    pub accounts: Vec<Account>,
    pub duplicates: Vec<Duplicate>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Duplicate {
    pub account: Account,
    pub of: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Declared {
    pub keepers: BTreeMap<String, String>,
    pub holders: Vec<(String, String)>,
}

pub enum Standing<'a> {
    Listed(&'a Account),
    Duplicate(&'a Duplicate),
    Unknown,
}

impl Roster {
    pub fn distinct(found: impl IntoIterator<Item = (Account, Option<String>)>, declared: &Declared) -> Self {
        let found: Vec<(Account, Option<String>)> = found.into_iter().collect();
        let mut holders: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        let fleet = found.iter().filter_map(|(account, identity)| Some((identity.as_deref()?, account.label.as_str())));
        let outside = declared.holders.iter().map(|(label, identity)| (identity.as_str(), label.as_str()));
        for (identity, label) in fleet.chain(outside) {
            let labels = holders.entry(identity).or_default();
            if !labels.contains(&label) {
                labels.push(label);
            }
        }
        let keeper = |identity: &str| -> String {
            let labels = &holders[identity];
            match declared.keepers.get(identity) {
                Some(label) if labels.contains(&label.as_str()) => label.clone(),
                _ => labels.iter().min().map(|label| (*label).to_owned()).unwrap_or_default(),
            }
        };
        let mut roster = Self::default();
        for (account, identity) in &found {
            match identity.as_deref().map(keeper).filter(|keeper| keeper != &account.label) {
                Some(of) => roster.duplicates.push(Duplicate { account: account.clone(), of }),
                None => roster.accounts.push(account.clone()),
            }
        }
        roster
    }

    pub fn merge(mut self, other: Self) -> Self {
        self.accounts.extend(other.accounts);
        self.duplicates.extend(other.duplicates);
        self
    }

    pub fn standing(&self, provider: &ProviderName, label: &str) -> Standing<'_> {
        let matches = |account: &Account| &account.provider == provider && account.label == label;
        if let Some(account) = self.accounts.iter().find(|account| matches(account)) {
            return Standing::Listed(account);
        }
        self.duplicates.iter().find(|duplicate| matches(&duplicate.account)).map_or(Standing::Unknown, Standing::Duplicate)
    }

    pub fn account(&self, provider: &ProviderName, label: &str) -> NorthResult<Account> {
        match self.standing(provider, label) {
            Standing::Listed(account) => Ok(account.clone()),
            Standing::Duplicate(duplicate) => Err(NorthError::Refused(format!(
                "{duplicate}; launch on another --account or log {label} in to its own account"
            ))),
            Standing::Unknown => Err(NorthError::Usage(format!("no {} account named {label}", provider.label()))),
        }
    }
}

impl fmt::Display for Duplicate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let provider = self.account.provider.label();
        write!(formatter, "{provider}:{} is logged in to the same account as {provider}:{}", self.account.label, self.of)
    }
}
