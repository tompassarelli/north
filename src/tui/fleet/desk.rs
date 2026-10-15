use std::collections::BTreeMap;
use std::sync::Arc;

use crate::error::NorthResult;
use crate::fleet::model::{Account, Delivery, LaunchRequest, SessionRecord, Status, Transcript};
use crate::fleet::registry::{Fleet, Follower};

#[derive(Clone, Debug, PartialEq)]
pub struct SessionSummary {
    pub record: SessionRecord,
    pub status: Status,
    pub last_activity_ms: Option<u64>,
}

pub trait SessionDesk {
    fn accounts(&self) -> Vec<Account>;
    fn refresh(&mut self) -> NorthResult<bool>;
    fn sessions(&self) -> Vec<SessionSummary>;
    fn transcript(&self, name: &str) -> Option<&Transcript>;
    fn actions(&self) -> Arc<dyn SessionActions>;
}

pub trait SessionActions: Send + Sync {
    fn launch(&self, request: &LaunchRequest) -> NorthResult<()>;
    fn send(&self, name: &str, text: &str, delivery: Delivery) -> NorthResult<()>;
    fn revive(&self, name: &str) -> NorthResult<()>;
    fn interrupt(&self, name: &str) -> NorthResult<()>;
    fn stop(&self, name: &str) -> NorthResult<()>;
    fn forget(&self, name: &str) -> NorthResult<()>;
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
}

pub struct FleetDesk {
    fleet: Arc<Fleet>,
    records: Vec<SessionRecord>,
    followers: BTreeMap<String, Follower>,
}

impl FleetDesk {
    pub fn new(fleet: Fleet) -> Self {
        Self { fleet: Arc::new(fleet), records: Vec::new(), followers: BTreeMap::new() }
    }

    fn follow_new(&mut self) -> NorthResult<bool> {
        let mut changed = false;
        for record in &self.records {
            if !self.followers.contains_key(&record.name) {
                self.followers.insert(record.name.clone(), self.fleet.follow(&record.name)?);
                changed = true;
            }
        }
        Ok(changed)
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
        Ok(self.poll_followers()? | listed | followed)
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
                }
            })
            .collect()
    }

    fn transcript(&self, name: &str) -> Option<&Transcript> {
        self.followers.get(name).map(Follower::transcript)
    }

    fn actions(&self) -> Arc<dyn SessionActions> {
        self.fleet.clone()
    }
}
