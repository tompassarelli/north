use std::collections::BTreeMap;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::error::{NorthError, NorthResult};
use crate::fleet::books::Digest;
use crate::fleet::gates::{Decision, due};
use crate::fleet::model::{Account, Delivery, LaunchRequest, SessionRecord, Status, Transcript};
use crate::foundation::clock::Timestamp;
use crate::fleet::registry::{Fleet, Follower};

use super::state_clock::StateClock;
use crate::fleet::runtime::now_ms;

const UPKEEP_INTERVAL: Duration = Duration::from_secs(30);
const GATE_RETRY: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq)]
pub struct SessionSummary {
    pub record: SessionRecord,
    pub status: Status,
    pub last_activity_ms: Option<u64>,
    pub since_ms: u64,
    pub depth: usize,
}

pub trait SessionDesk {
    fn accounts(&self) -> Vec<Account>;
    fn refresh(&mut self) -> NorthResult<bool>;
    fn sessions(&self) -> Vec<SessionSummary>;
    fn transcript(&self, name: &str) -> Option<&Transcript>;
    fn digest(&self) -> Digest;
    fn actions(&self) -> Arc<dyn SessionActions>;
}

pub trait SessionActions: Send + Sync {
    fn launch(&self, request: &LaunchRequest) -> NorthResult<()>;
    fn send(&self, name: &str, text: &str, delivery: Delivery) -> NorthResult<()>;
    fn revive(&self, name: &str) -> NorthResult<()>;
    fn interrupt(&self, name: &str) -> NorthResult<()>;
    fn stop(&self, name: &str) -> NorthResult<()>;
    fn forget(&self, name: &str) -> NorthResult<()>;
    fn decide(&self, gate: i64, decision: Decision) -> NorthResult<()>;
}

impl SessionActions for Fleet {
    fn launch(&self, request: &LaunchRequest) -> NorthResult<()> {
        Fleet::launch(self, request).map(drop)
    }

    fn send(&self, name: &str, text: &str, delivery: Delivery) -> NorthResult<()> {
        Fleet::send(self, name, text, delivery)
    }

    fn revive(&self, name: &str) -> NorthResult<()> {
        Fleet::revive(self, name, None).map(drop)
    }

    fn interrupt(&self, name: &str) -> NorthResult<()> {
        Fleet::interrupt(self, name)
    }

    fn stop(&self, name: &str) -> NorthResult<()> {
        Fleet::stop(self, name)
    }

    fn forget(&self, name: &str) -> NorthResult<()> {
        Fleet::forget(self, name)
    }

    fn decide(&self, gate: i64, decision: Decision) -> NorthResult<()> {
        Fleet::decide(self, gate, decision).map(drop)
    }
}

pub struct FleetDesk {
    fleet: Arc<Fleet>,
    records: Vec<SessionRecord>,
    followers: BTreeMap<String, Follower>,
    states: StateClock,
    digest: Digest,
    upkeep: Upkeep,
}

impl FleetDesk {
    pub fn new(fleet: Fleet) -> Self {
        Self { fleet: Arc::new(fleet), records: Vec::new(), followers: BTreeMap::new(), states: StateClock::default(), digest: Digest::default(), upkeep: Upkeep::default() }
    }

    fn follow_new(&mut self) -> NorthResult<bool> {
        let mut changed = false;
        for record in &self.records {
            if !self.followers.contains_key(&record.name) {
                self.followers.insert(record.name.clone(), self.fleet.follow_recent(&record.name)?);
                changed = true;
            }
        }
        Ok(changed)
    }

    fn track_states(&mut self) {
        let now = now_ms();
        let followers = &self.followers;
        self.states.retain(|name| followers.contains_key(name));
        for record in &self.records {
            let Some(follower) = self.followers.get(&record.name) else { continue };
            let transcript = follower.transcript();
            let first_seen = transcript.turns.last().map_or(record.created_ms, |turn| turn.ended_ms);
            self.states.observe(&record.name, transcript.status, first_seen, now);
        }
    }

    fn poll_followers(&mut self) -> NorthResult<bool> {
        let mut changed = false;
        for follower in self.followers.values_mut() {
            changed |= follower.poll()?;
        }
        Ok(changed)
    }
}

impl SessionDesk for FleetDesk {
    fn accounts(&self) -> Vec<Account> {
        self.fleet.accounts()
    }

    fn refresh(&mut self) -> NorthResult<bool> {
        let records = self.fleet.sessions()?;
        let listed = records != self.records;
        self.records = records;
        let records = &self.records;
        self.followers.retain(|name, _| records.iter().any(|record| &record.name == name));
        let followed = self.follow_new()?;
        let digest = self.fleet.digest(Timestamp::now().local_day_start_ms())?;
        let booked = digest != self.digest;
        self.digest = digest;
        let changed = self.poll_followers()? | listed | followed | booked;
        self.track_states();
        self.upkeep.tend(&self.fleet, &self.digest)?;
        Ok(changed)
    }

    fn sessions(&self) -> Vec<SessionSummary> {
        self.records
            .iter()
            .map(|record| {
                let transcript = self.followers.get(&record.name).map(Follower::transcript);
                SessionSummary {
                    record: record.clone(),
                    status: transcript.map_or(Status::Starting, |transcript| transcript.status),
                    last_activity_ms: transcript.and_then(|transcript| transcript.last_activity_ms),
                    since_ms: self.states.since(&record.name).unwrap_or(record.created_ms),
                    depth: 0,
                }
            })
            .collect()
    }

    fn transcript(&self, name: &str) -> Option<&Transcript> {
        self.followers.get(name).map(Follower::transcript)
    }

    fn digest(&self) -> Digest {
        self.digest.clone()
    }

    fn actions(&self) -> Arc<dyn SessionActions> {
        self.fleet.clone()
    }
}

#[derive(Default)]
struct Upkeep {
    job: Option<JoinHandle<NorthResult<()>>>,
    last: Option<Instant>,
}

impl Upkeep {
    fn tend(&mut self, fleet: &Arc<Fleet>, digest: &Digest) -> NorthResult<()> {
        if let Some(job) = self.job.take_if(|job| job.is_finished()) {
            job.join().map_err(|_| NorthError::State("fleet upkeep panicked".into()))??;
        }
        let now = now_ms();
        let gate_due = digest.open_gates().any(|gate| due(gate, now));
        let wait = if gate_due { GATE_RETRY } else { UPKEEP_INTERVAL };
        if self.job.is_none() && self.last.is_none_or(|last| last.elapsed() >= wait) {
            let fleet = fleet.clone();
            self.job = Some(thread::spawn(move || upkeep(&fleet)));
            self.last = Some(Instant::now());
        }
        Ok(())
    }
}

fn upkeep(fleet: &Fleet) -> NorthResult<()> {
    let undelivered = fleet.default_overdue_gates()?.into_iter().find_map(|(_, outcome)| outcome.err());
    fleet.observe_landings()?;
    undelivered.map_or(Ok(()), Err)
}
