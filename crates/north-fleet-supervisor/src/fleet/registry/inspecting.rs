//! Reading the fleet: the sessions it holds, the state each is in, and what it waits on.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::error::NorthResult;
use crate::fleet::detached;
use crate::fleet::jobs::SpooledLeases;
use crate::fleet::lane_probe;
use crate::fleet::model::{Account, Phases, ProviderName, SessionRecord, Status};
use crate::fleet::prompt::StandingOrders;
use crate::fleet::records::read_phased;
use crate::fleet::roster::Roster;
use crate::fleet::store::{OverSsh, Spool};
use crate::fleet::tickets;
use crate::fleet::waits::Waits;
use crate::fleet::watch::TurnWatch;

use super::home::state_dir;
use super::{Fleet, session_running};

impl Fleet {
    pub fn watch(&self, provider: &ProviderName, host_dir: &Path) -> NorthResult<TurnWatch> {
        Ok(TurnWatch::new(self.runtimes.get(provider)?, self.ledger.clone(), Arc::new(self.clone()), self.claims.clone(), self.root.clone(), host_dir.to_path_buf()).reading_lanes(Arc::new(lane_probe::Git)).writing_usage(self.usage.clone()).recording_runs(self.recording.clone()).reckoning_with(self.reckon.clone()).reading_tickets(Arc::new(tickets::LandQueue::here())).reading_jobs(Arc::new(SpooledLeases::new(self.leases.clone(), Spool::at(&self.root)).reading_far(Arc::new(OverSsh::default())))).reading_scope(Arc::new(detached::Procfs::here().after(detached::Prior::all(host_dir)))))
    }

    /// Every phase move of this host's sessions, in the lifecycle store beside the fleet root.
    pub fn phases(&self) -> NorthResult<Phases> {
        Phases::open(&self.root)
    }

    pub fn waits(&self) -> Waits {
        Waits::in_root(&self.root)
    }

    pub fn orders(&self) -> StandingOrders {
        StandingOrders::in_state(&self.state)
    }

    pub fn roster(&self) -> Roster {
        self.runtimes.roster(&state_dir(&self.home), &self.home, &self.policy)
    }

    pub fn placed_hosts(&self) -> Option<Vec<String>> {
        crate::platform::orchestration::placed_hosts(&self.policy)
    }

    pub fn owners_machine(&self) -> Option<String> {
        crate::platform::orchestration::owners_machine(&self.policy)
    }

    pub fn accounts(&self) -> Vec<Account> {
        self.roster().accounts
    }

    pub fn providers(&self) -> Vec<ProviderName> {
        self.runtimes.providers()
    }

    pub fn sessions(&self) -> NorthResult<Vec<SessionRecord>> {
        let mut phases = Phases::reading(&self.root);
        let mut sessions: Vec<SessionRecord> = fs::read_dir(&self.root)?
            .flatten()
            .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
            .filter_map(|entry| read_phased(phases.as_mut(), &entry.path()).ok())
            .collect();
        sessions.sort_by_key(|record| record.created_ms);
        Ok(sessions)
    }

    pub fn session(&self, name: &str) -> NorthResult<SessionRecord> {
        self.record(name)
    }

    /// A session is running while a live host serves it, or, adopted, while its interactive window lives.
    pub fn running(&self, record: &SessionRecord) -> bool {
        if record.adopted() {
            return self.window_status(record) != Status::Exited;
        }
        session_running(record)
    }
}
