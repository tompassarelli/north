//! The outcome watchdog: each critical flow declares the outcome it must keep producing and how stale that outcome
//! may grow, and `north health` judges every flow from what it finds recorded, never from the writer's exit code.
//! A flow that stops alarms the proxy through health's episode alerts, with the flow, the age and the first cause.
use serde::{Deserialize, Serialize};

use north_base::lifecycle::Fault;

use crate::foundation::clock::Timestamp;

pub mod facts;
pub mod probe;

#[cfg(test)]
mod tests;

/// Worker activity newer than the ledger's newest recorded run by more than this means runs stopped reaching it.
const RECORDING_LAG_S: f64 = 10.0 * 60.0;
/// One snapshot pull interval and five minutes more for a slow pull.
const SNAPSHOT_AGE_S: f64 = crate::threads::snapshot::SNAPSHOT_SECONDS as f64 + 5.0 * 60.0;
/// A landing not live on a host this long after it reached origin/main is stuck.
const LANDING_LAG_S: f64 = 10.0 * 60.0;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Held {
    pub since: f64,
    pub cause: String,
}

/// What this host's lifecycle store says is stuck, or why it could not be read.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Lifecycles {
    Read(Vec<Fault>),
    Unreadable(String),
}

impl Default for Lifecycles {
    fn default() -> Self {
        Self::Read(Vec::new())
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Dispatch {
    pub timer_active: bool,
    pub paused: Option<Held>,
    pub last_run: Option<f64>,
}

/// What one host records about its own flows, printed by `north health facts` there.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct HostFacts {
    pub host: String,
    pub runtime: Option<String>,
    pub policy_held: Option<Held>,
    pub policy_published: Option<f64>,
    pub unhooked: Vec<String>,
    pub dispatch: Option<Dispatch>,
    pub newest_run: Option<f64>,
    pub newest_activity: Option<f64>,
    pub lifecycles: Lifecycles,
}

/// One host as the laptop saw it: its facts, or why they could not be read, and how far its runtime trails main.
#[derive(Clone, Debug, PartialEq)]
pub struct Host {
    pub name: String,
    pub facts: Result<HostFacts, String>,
    /// When the oldest origin/main commit this host's runtime lacks landed, `None` when it runs main.
    pub behind_since: Result<Option<f64>, String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snapshot {
    pub written: Option<f64>,
    pub failure: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct World {
    pub now: f64,
    pub hosts: Vec<Host>,
    /// The ledger snapshot this host reads, `None` on the ledger host, which reads the ledger itself.
    pub snapshot: Option<Snapshot>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    pub name: &'static str,
    /// Seconds since the flow's newest outcome, `None` when it records no time.
    pub age_s: Option<f64>,
    pub verdict: Result<(), String>,
}

pub fn minutes(seconds: f64) -> String {
    let minutes = (seconds.max(0.0) / 60.0).round() as u64;
    if minutes < 60 { format!("{minutes} min") } else { format!("{} h {} min", minutes / 60, minutes % 60) }
}

fn flow(name: &'static str, age_s: Option<f64>, faults: Vec<String>) -> Flow {
    Flow { name, age_s, verdict: if faults.is_empty() { Ok(()) } else { Err(faults.join("; ")) } }
}

impl World {
    fn reached(&self) -> impl Iterator<Item = (&str, &HostFacts)> {
        self.hosts.iter().filter_map(|host| host.facts.as_ref().ok().map(|facts| (host.name.as_str(), facts)))
    }

    fn age(&self, at: f64) -> f64 {
        self.now - at
    }

    pub fn judge(&self) -> Vec<Flow> {
        vec![self.probes(), self.recording(), self.snapshot(), self.policy(), self.hooks(), self.dispatch(), self.landing(), self.lifecycles()]
    }

    fn probes(&self) -> Flow {
        let faults = self.hosts.iter().filter_map(|host| host.facts.as_ref().err().map(|why| format!("{}: facts unreadable: {why}", host.name))).collect();
        flow("host-probes", Some(0.0), faults)
    }

    fn recording(&self) -> Flow {
        let newest_run = self.reached().filter_map(|(_, facts)| facts.newest_run).reduce(f64::max);
        let activity = self.reached().filter_map(|(name, facts)| Some((name, facts.newest_activity?))).max_by(|a, b| a.1.total_cmp(&b.1));
        let mut faults = Vec::new();
        match (newest_run, activity) {
            (None, _) => faults.push("no host reports the ledger's newest recorded run".to_owned()),
            (Some(run), Some((name, active))) if active - run > RECORDING_LAG_S => faults.push(format!(
                "the ledger's newest recorded run ended {} ago while workers ran {} later (newest on {name}, {} ago)",
                minutes(self.age(run)),
                minutes(active - run),
                minutes(self.age(active))
            )),
            _ => {}
        }
        flow("run-recording", newest_run.map(|run| self.age(run)), faults)
    }

    fn snapshot(&self) -> Flow {
        let Some(snapshot) = &self.snapshot else { return flow("snapshot", None, Vec::new()) };
        let age = snapshot.written.map(|written| self.age(written));
        let mut faults = Vec::new();
        match age {
            None => faults.push("the ledger snapshot is missing".to_owned()),
            Some(age) if age > SNAPSHOT_AGE_S => faults.push(format!("the ledger snapshot was last written {} ago", minutes(age))),
            _ => {}
        }
        if let Some(why) = &snapshot.failure {
            faults.push(format!("its last pull failed: {why}"));
        }
        flow("snapshot", age, faults)
    }

    fn policy(&self) -> Flow {
        let mut faults = Vec::new();
        let mut age: Option<f64> = None;
        for (name, facts) in self.reached() {
            if let Some(held) = &facts.policy_held {
                faults.push(format!("{name} has published no policy generation for {}: {}", minutes(self.age(held.since)), held.cause));
            }
            let since = facts.policy_held.as_ref().map(|held| held.since).or(facts.policy_published);
            age = since.map(|since| self.age(since)).into_iter().chain(age).reduce(f64::max);
        }
        flow("policy", age, faults)
    }

    fn hooks(&self) -> Flow {
        let faults = self.reached().flat_map(|(name, facts)| facts.unhooked.iter().map(move |why| format!("{name} launches without hooks in {why}"))).collect();
        flow("hooks", Some(0.0), faults)
    }

    fn dispatch(&self) -> Flow {
        let mut faults = Vec::new();
        let mut age: Option<f64> = None;
        for (name, facts) in self.reached() {
            let Some(dispatch) = &facts.dispatch else { continue };
            age = dispatch.last_run.map(|run| self.age(run)).into_iter().chain(age).reduce(f64::max);
            match (&dispatch.paused, dispatch.timer_active) {
                (Some(paused), true) => faults.push(format!("{name} runs the dispatch timer though the proxy paused dispatch {} ago: {}", minutes(self.age(paused.since)), paused.cause)),
                (None, false) => faults.push(format!("{name} has its dispatch timer stopped with no pause recorded (`north dispatch pause --reason TEXT` records one)")),
                _ => {}
            }
        }
        flow("dispatch", age, faults)
    }

    fn landing(&self) -> Flow {
        let mut faults = Vec::new();
        let mut age: f64 = 0.0;
        for host in self.hosts.iter().filter(|host| host.facts.is_ok()) {
            match &host.behind_since {
                Err(why) => faults.push(format!("{}: {why}", host.name)),
                Ok(None) => {}
                Ok(Some(since)) => {
                    age = age.max(self.age(*since));
                    if self.age(*since) > LANDING_LAG_S {
                        faults.push(format!("{} runs a runtime without a landing on origin/main for {}", host.name, minutes(self.age(*since))));
                    }
                }
            }
        }
        flow("landing", Some(age), faults)
    }

    /// Every explicit state machine on every host, judged from its records: a state past its dwell, a state whose
    /// holder died, or a move the machine does not have.
    fn lifecycles(&self) -> Flow {
        let now = Timestamp::from_unix_seconds_f64(self.now);
        let faults = self
            .reached()
            .flat_map(|(name, facts)| match &facts.lifecycles {
                Lifecycles::Read(faults) => faults.iter().map(|fault| format!("{name}: {}", fault.line(now))).collect(),
                Lifecycles::Unreadable(why) => vec![format!("{name}: its lifecycle store is unreadable: {why}")],
            })
            .collect();
        flow("state-machines", Some(0.0), faults)
    }
}
