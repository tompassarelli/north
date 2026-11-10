//! Free worker slots: a host has its resident cap minus the sessions it runs, no more than the builds
//! its admission queue takes. Whether an account may take a worker is the spend rule's call
//! (`crate::spend`), which counts the workers this run has started.
pub use crate::pacing::placement::room::{Charge, PlanSize, Room};
use crate::spend::{Placed, Rules, Starter, View};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Host {
    pub name: String,
    pub cap: Option<i64>,
    /// Running resident sessions, or why the host takes nothing now.
    pub running: Result<i64, String>,
    /// Its live load, when dispatch reaches it.
    pub measure: Option<super::measure::Measure>,
    /// Builds its capacity admission takes before its queue reaches the limit placement skips it at,
    /// None without a backlog reading; each launch this run is expected to queue one.
    pub builds: Option<i64>,
}

impl Host {
    /// The host as dispatch may use it: a saturated one (full admission queue, low disk, high load)
    /// takes nothing, so its boxes fall through to the next host in the placement order.
    pub fn saturated(self, why: Option<String>) -> Self {
        match why {
            Some(why) if self.running.is_ok() => Self { running: Err(why), ..self },
            _ => self,
        }
    }

    /// The host with the saturation reading it reports, read when dispatch reaches it (here or over
    /// ssh); an unreadable reading refuses nothing.
    pub fn probed(self, here: &str, ssh: Option<&str>) -> Self {
        let reachable = self.name == here || ssh.is_some();
        let reading = reachable.then(|| crate::pacing::placement::saturation_reading(&self.name).ok()).flatten();
        self.read(reading.as_ref())
    }

    /// The host refused by the reading's memory, root disk or admission queue, with the builds its
    /// admission queue has room for.
    pub fn read(self, reading: Option<&crate::capacity::saturation::Saturation>) -> Self {
        let builds = reading.and_then(|reading| reading.backlog).map(|backlog| backlog.build_room().max(0));
        let why = reading.and_then(|reading| reading.refusal(&self.name));
        Self { builds, ..self }.saturated(why)
    }

    pub fn free(&self) -> Result<i64, String> {
        let running = self.running.clone()?;
        let cap = self.cap.ok_or_else(|| format!("no [hosts.{}] resident_cap", self.name))?;
        Ok((cap - running).max(0))
    }

    pub fn line(&self) -> String {
        let measured = self.measure.map_or_else(String::new, |measure| format!("; load {:.1} on {:.0} cores, {:.0} of {:.0} MiB available", measure.load, measure.cores, measure.memory_available_mib, measure.memory_total_mib));
        let state = match (&self.running, self.cap) {
            (Err(why), _) => format!("host {}: takes nothing: {why}", self.name),
            (Ok(running), None) => format!("host {}: {running} running, no resident_cap, so 0 free", self.name),
            (Ok(running), Some(cap)) => format!("host {}: {running}/{cap} resident, {} free", self.name, (cap - running).max(0)),
        };
        let builds = self.builds.map_or_else(String::new, |builds| format!("; its admission queue takes {builds} more builds"));
        format!("{state}{builds}{measured}")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    /// `provider:label`, as the usage readings name it.
    pub name: String,
    /// Hosts the account is signed in on.
    pub hosts: Vec<String>,
    /// Plan size per window in Max 5x units (`[accounts.PROVIDER] plans`), which puts every account's percent on one scale.
    pub plan: PlanSize,
    pub room: Room,
    /// The deadline rank of an API grant, which has no plan windows; None for a plan account.
    pub grant: Option<crate::pacing::placement::room::Rank>,
}

impl Account {
    pub fn provider(&self) -> &str {
        self.name.split_once(':').map_or(self.name.as_str(), |(provider, _)| provider)
    }

    pub fn line(&self, spend: &View, rules: &Rules) -> String {
        let state = match spend.admit(rules, &self.name, Starter::Reserved, Placed::default()) {
            Ok(()) => spend.trips.iter().find(|trip| trip.account == self.name).map_or_else(|| "open".to_owned(), |trip| format!("tripped: {}", trip.line)),
            Err(refusal) => format!("out: {refusal}"),
        };
        let room = &self.room;
        format!(
            "account {}: {state}; plan {} x Max 5x in the 5h window and {} x in the week; {:.0} points left in the 5h window, {:.0} in the week, which resets in {:.0} h; signed in on {}",
            self.name,
            self.plan.five_hour,
            self.plan.week,
            room.five_hour,
            room.week,
            room.week_reset_h,
            self.hosts.join(", ")
        )
    }
}

/// What is left to hand out this run, taken from as launches are planned.
#[derive(Clone, Debug, PartialEq)]
pub struct Pools {
    hosts: BTreeMap<String, i64>,
    /// Builds each host's admission queue still takes this run, for hosts with a backlog reading.
    builds: BTreeMap<String, i64>,
    /// The hosts each account is signed in on.
    signed_in: BTreeMap<String, Vec<String>>,
    /// Points of its own percent each account is charged for the boxes planned on it this run, which orders the next pick.
    charged: BTreeMap<String, Charge>,
    /// The workers this run has started, which the spend rule's reserve counts.
    placed: Placed,
    cloud: i64,
    /// The opencode workers this run has started, which count against their own slot cap.
    deepseek: usize,
}

impl Pools {
    pub fn new(hosts: &[Host], accounts: &[Account]) -> Self {
        Self {
            hosts: hosts.iter().map(|host| (host.name.clone(), host.free().unwrap_or(0))).collect(),
            builds: hosts.iter().filter_map(|host| Some((host.name.clone(), host.builds?))).collect(),
            signed_in: accounts.iter().map(|account| (account.name.clone(), account.hosts.clone())).collect(),
            charged: BTreeMap::new(),
            placed: Placed::default(),
            cloud: 0,
            deepseek: 0,
        }
    }

    pub fn with_cloud(self, free: i64) -> Self {
        Self { cloud: free.max(0), ..self }
    }

    pub fn take_cloud(&mut self) -> bool {
        let free = self.cloud > 0;
        self.cloud -= i64::from(free);
        free
    }

    pub fn deepseek_placed(&self) -> usize {
        self.deepseek
    }

    pub fn take_deepseek(&mut self) {
        self.deepseek += 1;
    }

    fn serves(&self, host: &str, account: &str) -> bool {
        self.signed_in.get(account).is_some_and(|hosts| hosts.iter().any(|name| name == host))
    }

    /// The host's free slots when the account is signed in there, else 0.
    pub fn pair(&self, host: &str, account: &str) -> i64 {
        if self.serves(host, account) { self.host(host) } else { 0 }
    }

    /// What this run has started so far.
    pub fn placed(&self) -> Placed {
        self.placed
    }

    /// The host's free session slots this run, no more than the builds its admission queue takes.
    pub fn host(&self, host: &str) -> i64 {
        self.hosts.get(host).copied().unwrap_or(0).min(self.builds.get(host).copied().unwrap_or(i64::MAX))
    }

    /// Why the host takes no more boxes this run, once `host` reads 0.
    pub fn full(&self, host: &str) -> String {
        match self.builds.get(host) {
            Some(0) => format!("{host} admission queue has no room for another build this run"),
            _ => format!("{host} has no free slot"),
        }
    }

    pub fn total(&self) -> i64 {
        self.hosts.keys().filter(|host| self.signed_in.keys().any(|account| self.serves(host, account))).map(|host| self.host(host)).sum()
    }

    pub fn take(&mut self, host: &str) {
        self.take_slot(host);
        self.placed.workers += 1;
    }

    /// Takes a session slot and a build on `host` for a worker the Claude spend rule does not count.
    pub fn take_slot(&mut self, host: &str) {
        for free in self.hosts.get_mut(host).into_iter().chain(self.builds.get_mut(host)) {
            *free = (*free - 1).max(0);
        }
    }

    /// The account's room less what this run has charged it.
    pub fn room(&self, account: &Account) -> Room {
        account.room.less(self.charged.get(&account.name).copied().unwrap_or_default())
    }

    pub fn charge(&mut self, account: &str, charge: Charge) {
        let total = self.charged.entry(account.into()).or_default();
        total.five_hour += charge.five_hour;
        total.week += charge.week;
    }
}
