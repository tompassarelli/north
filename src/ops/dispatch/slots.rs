//! Free worker slots: a host has its resident cap minus the sessions it runs, and an account the
//! out-rule keeps out has none; any other account may take every free slot of a host it is signed in on.
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Host {
    pub name: String,
    pub cap: Option<i64>,
    /// Running resident sessions, or why the host takes nothing now.
    pub running: Result<i64, String>,
    /// Its live load, when dispatch reaches it.
    pub measure: Option<super::measure::Measure>,
}

impl Host {
    pub(crate) fn free(&self) -> Result<i64, String> {
        let running = self.running.clone()?;
        let cap = self.cap.ok_or_else(|| format!("no [hosts.{}] resident_cap", self.name))?;
        Ok((cap - running).max(0))
    }

    pub(crate) fn line(&self) -> String {
        let measured = self.measure.map_or_else(String::new, |measure| format!("; load {:.1} on {:.0} cores, {:.0} of {:.0} MiB available", measure.load, measure.cores, measure.memory_available_mib, measure.memory_total_mib));
        let state = match (&self.running, self.cap) {
            (Err(why), _) => format!("host {}: takes nothing: {why}", self.name),
            (Ok(running), None) => format!("host {}: {running} running, no resident_cap, so 0 free", self.name),
            (Ok(running), Some(cap)) => format!("host {}: {running}/{cap} resident, {} free", self.name, (cap - running).max(0)),
        };
        format!("{state}{measured}")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Account {
    /// `provider:label`, as pace-control names it.
    pub name: String,
    /// Hosts the account is signed in on.
    pub hosts: Vec<String>,
    /// Plan size per window in Max 5x units (`[accounts.PROVIDER] plans`), which puts every account's percent on one scale.
    pub plan: PlanSize,
    pub room: Room,
    /// Whether a pace spend target (`[pace."A"] spend_to_pct` by `by`) is live for the account.
    pub spending: bool,
    /// Why the out-rule keeps the account out, if it does.
    pub refusal: Option<String>,
}

/// What a plan holds of each window, in Max 5x units: Max 20x holds four times the five-hour room of Max 5x but only twice its week.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PlanSize {
    pub five_hour: f64,
    pub week: f64,
}

/// Points of an account's own percent a launch takes from each window.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Charge {
    pub five_hour: f64,
    pub week: f64,
}

/// Points an account has left, in its own percent, before the five-hour stop and before the week is exhausted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Room {
    pub five_hour: f64,
    pub week: f64,
    /// Hours until the week resets, when its unspent room is lost.
    pub week_reset_h: f64,
}

impl Room {
    const SOONEST_RESET_H: f64 = 1.0 / 60.0;

    /// Week room in plan units per hour until it resets: the capacity lost each hour it is not spent.
    pub(crate) fn expiring(&self, plan: PlanSize) -> f64 {
        self.week * plan.week / self.week_reset_h.max(Self::SOONEST_RESET_H)
    }

    pub(crate) fn less(self, charge: Charge) -> Self {
        Self { five_hour: self.five_hour - charge.five_hour, week: self.week - charge.week, ..self }
    }

    /// Whether `charge` more leaves both windows short of the point where the out-rule refuses.
    pub(crate) fn takes(&self, charge: Charge) -> bool {
        let after = self.less(charge);
        after.five_hour > 0.0 && after.week > 0.0
    }
}

impl Account {
    pub(crate) fn open(&self) -> Result<(), String> {
        self.refusal.clone().map_or(Ok(()), Err)
    }

    pub(crate) fn provider(&self) -> &str {
        self.name.split_once(':').map_or(self.name.as_str(), |(provider, _)| provider)
    }

    pub(crate) fn line(&self) -> String {
        let state = match self.open() {
            Ok(()) => "open".to_owned(),
            Err(why) => format!("out: {why}"),
        };
        let room = &self.room;
        format!(
            "account {}: {state}; plan {} x Max 5x in the 5h window and {} x in the week; {:.0} points to the 5h stop, {:.0} left in the week, which resets in {:.0} h; signed in on {}",
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
pub(crate) struct Pools {
    hosts: BTreeMap<String, i64>,
    /// The hosts each open account is signed in on.
    signed_in: BTreeMap<String, Vec<String>>,
    /// Points of its own percent each account is charged for the boxes planned on it this run.
    charged: BTreeMap<String, Charge>,
    cloud: i64,
}

impl Pools {
    pub(crate) fn new(hosts: &[Host], accounts: &[Account]) -> Self {
        Self {
            hosts: hosts.iter().map(|host| (host.name.clone(), host.free().unwrap_or(0))).collect(),
            signed_in: accounts.iter().filter(|account| account.open().is_ok()).map(|account| (account.name.clone(), account.hosts.clone())).collect(),
            charged: BTreeMap::new(),
            cloud: 0,
        }
    }

    pub(crate) fn with_cloud(self, free: i64) -> Self {
        Self { cloud: free.max(0), ..self }
    }

    pub(crate) fn take_cloud(&mut self) -> bool {
        let free = self.cloud > 0;
        self.cloud -= i64::from(free);
        free
    }

    fn serves(&self, host: &str, account: &str) -> bool {
        self.signed_in.get(account).is_some_and(|hosts| hosts.iter().any(|name| name == host))
    }

    /// The host's free slots when the account is open and signed in there, else 0.
    pub(crate) fn pair(&self, host: &str, account: &str) -> i64 {
        if self.serves(host, account) { self.host(host) } else { 0 }
    }

    pub(crate) fn host(&self, host: &str) -> i64 {
        self.hosts.get(host).copied().unwrap_or(0)
    }

    pub(crate) fn total(&self) -> i64 {
        self.hosts.iter().filter(|(host, _)| self.signed_in.keys().any(|account| self.serves(host, account))).map(|(_, free)| free).sum()
    }

    pub(crate) fn take(&mut self, host: &str) {
        if let Some(free) = self.hosts.get_mut(host) {
            *free = (*free - 1).max(0);
        }
    }

    /// The account's room less what this run has charged it.
    pub(crate) fn room(&self, account: &Account) -> Room {
        account.room.less(self.charged.get(&account.name).copied().unwrap_or_default())
    }

    pub(crate) fn charge(&mut self, account: &str, charge: Charge) {
        let total = self.charged.entry(account.into()).or_default();
        total.five_hour += charge.five_hour;
        total.week += charge.week;
    }
}
