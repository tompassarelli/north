//! Free worker slots: a host has its resident cap minus the sessions it runs, an account its
//! pace-control allowed_workers minus the sessions it runs, and a pair the smaller of the two.
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Host {
    pub name: String,
    pub cap: Option<i64>,
    /// Running resident sessions, or why the host takes nothing now.
    pub running: Result<i64, String>,
}

impl Host {
    pub(crate) fn free(&self) -> Result<i64, String> {
        let running = self.running.clone()?;
        let cap = self.cap.ok_or_else(|| format!("no [hosts.{}] resident_cap", self.name))?;
        Ok((cap - running).max(0))
    }

    pub(crate) fn line(&self) -> String {
        match (&self.running, self.cap) {
            (Err(why), _) => format!("host {}: takes nothing: {why}", self.name),
            (Ok(running), None) => format!("host {}: {running} running, no resident_cap, so 0 free", self.name),
            (Ok(running), Some(cap)) => format!("host {}: {running}/{cap} resident, {} free", self.name, (cap - running).max(0)),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Account {
    /// `provider:label`, as pace-control names it.
    pub name: String,
    pub allowed: Option<i64>,
    pub running: i64,
    /// Hosts the account is signed in on.
    pub hosts: Vec<String>,
    pub headroom: f64,
    /// Why placement or pace-control keeps the account out, if it does.
    pub refusal: Option<String>,
}

impl Account {
    pub(crate) fn free(&self) -> Result<i64, String> {
        if let Some(why) = &self.refusal {
            return Err(why.clone());
        }
        let allowed = self.allowed.ok_or("no allowed_workers in pace-control.json")?;
        Ok((allowed - self.running).max(0))
    }

    pub(crate) fn provider(&self) -> &str {
        self.name.split_once(':').map_or(self.name.as_str(), |(provider, _)| provider)
    }

    pub(crate) fn line(&self) -> String {
        let free = match self.free() {
            Ok(free) => format!("{free} free"),
            Err(why) => format!("0 free: {why}"),
        };
        let allowed = self.allowed.map_or_else(|| "-".to_owned(), |allowed| allowed.to_string());
        format!("account {}: allowed {allowed}, running {}, {free}; signed in on {}", self.name, self.running, self.hosts.join(", "))
    }
}

/// What is left to hand out this run, taken from as launches are planned.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Pools {
    hosts: BTreeMap<String, i64>,
    accounts: BTreeMap<String, i64>,
    signed_in: BTreeMap<String, Vec<String>>,
}

impl Pools {
    pub(crate) fn new(hosts: &[Host], accounts: &[Account]) -> Self {
        Self {
            hosts: hosts.iter().map(|host| (host.name.clone(), host.free().unwrap_or(0))).collect(),
            accounts: accounts.iter().map(|account| (account.name.clone(), account.free().unwrap_or(0))).collect(),
            signed_in: accounts.iter().map(|account| (account.name.clone(), account.hosts.clone())).collect(),
        }
    }

    /// min(host free, account free) when the account is signed in on the host, else 0.
    pub(crate) fn pair(&self, host: &str, account: &str) -> i64 {
        if !self.signed_in.get(account).is_some_and(|hosts| hosts.iter().any(|name| name == host)) {
            return 0;
        }
        self.hosts.get(host).copied().unwrap_or(0).min(self.accounts.get(account).copied().unwrap_or(0))
    }

    pub(crate) fn host(&self, host: &str) -> i64 {
        self.hosts.get(host).copied().unwrap_or(0)
    }

    pub(crate) fn total(&self) -> i64 {
        let mut total = 0;
        let mut accounts = self.accounts.clone();
        for (host, free) in &self.hosts {
            let mut left = *free;
            for (account, room) in accounts.iter_mut() {
                if self.signed_in[account].iter().any(|name| name == host) {
                    let take = left.min(*room);
                    (left, *room, total) = (left - take, *room - take, total + take);
                }
            }
        }
        total
    }

    pub(crate) fn take(&mut self, host: &str, account: &str) {
        for pool in [self.hosts.get_mut(host), self.accounts.get_mut(account)].into_iter().flatten() {
            *pool = (*pool - 1).max(0);
        }
    }
}
