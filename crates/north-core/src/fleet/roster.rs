use std::collections::BTreeMap;
use std::fmt;

use crate::error::{NorthError, NorthResult};

use super::model::{Account, ProviderName};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Roster {
    pub accounts: Vec<Account>,
    pub duplicates: Vec<Duplicate>,
    pub foreign: Vec<Foreign>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Duplicate {
    pub account: Account,
    pub of: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Foreign {
    pub account: Account,
    pub owners: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Declared {
    pub keepers: BTreeMap<String, String>,
    pub holders: Vec<(String, String)>,
    pub elsewhere: BTreeMap<String, Vec<String>>,
}

pub enum Standing<'a> {
    Listed(&'a Account),
    Duplicate(&'a Duplicate),
    Foreign(&'a Foreign),
    Unknown,
}

impl Roster {
    pub fn distinct(found: impl IntoIterator<Item = (Account, Option<String>)>, declared: &Declared) -> Self {
        let mut roster = Self::default();
        let mut found: Vec<(Account, Option<String>)> = found.into_iter().collect();
        found.retain(|(account, _)| match declared.elsewhere.get(&account.label) {
            Some(owners) => {
                roster.foreign.push(Foreign { account: account.clone(), owners: owners.clone() });
                false
            }
            None => true,
        });
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
        self.foreign.extend(other.foreign);
        self
    }

    pub fn omitted(&self) -> impl Iterator<Item = String> + '_ {
        self.duplicates.iter().map(ToString::to_string).chain(self.foreign.iter().map(ToString::to_string))
    }

    pub fn standing(&self, provider: &ProviderName, label: &str) -> Standing<'_> {
        let matches = |account: &Account| &account.provider == provider && account.label == label;
        if let Some(account) = self.accounts.iter().find(|account| matches(account)) {
            return Standing::Listed(account);
        }
        if let Some(duplicate) = self.duplicates.iter().find(|duplicate| matches(&duplicate.account)) {
            return Standing::Duplicate(duplicate);
        }
        self.foreign.iter().find(|foreign| matches(&foreign.account)).map_or(Standing::Unknown, Standing::Foreign)
    }

    pub fn account(&self, provider: &ProviderName, label: &str) -> NorthResult<Account> {
        match self.standing(provider, label) {
            Standing::Listed(account) => Ok(account.clone()),
            Standing::Duplicate(duplicate) => Err(NorthError::Refused(format!(
                "{duplicate}; launch on another --account or log {label} in to its own account"
            ))),
            Standing::Foreign(foreign) => Err(NorthError::Refused(format!("{foreign}; launch on {} or another --account", foreign.owners.join(" or ")))),
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

impl fmt::Display for Foreign {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let owners = self.owners.join(" and ");
        write!(formatter, "{}:{}'s login belongs to {owners}, and refreshing a copy of it here would revoke {owners}'s", self.account.provider.label(), self.account.label)
    }
}
