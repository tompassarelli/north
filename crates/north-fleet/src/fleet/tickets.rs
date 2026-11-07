//! The landing tickets a session enqueued that have not landed, which hold the session listed.
use std::path::PathBuf;

use north_base::land_ticket::{self, Phase};

use crate::error::NorthResult;

pub trait Tickets: Send + Sync {
    /// The branches whose newest ticket from this session has not landed.
    fn unlanded(&self, session: &str) -> NorthResult<Vec<String>>;
}

pub struct NoTickets;

impl Tickets for NoTickets {
    fn unlanded(&self, _session: &str) -> NorthResult<Vec<String>> {
        Ok(Vec::new())
    }
}

/// The lander's queues on this machine: one directory per repository, its pending tickets at the top and its settled ones in areas below.
pub struct LandQueue {
    root: PathBuf,
}

impl LandQueue {
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn here() -> Self {
        let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).filter(|path| path.is_absolute());
        let state = state.unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
        Self::at(state.join("north/land-queue"))
    }
}

impl Tickets for LandQueue {
    fn unlanded(&self, session: &str) -> NorthResult<Vec<String>> {
        Ok(land_ticket::lanes_of(&self.root, session)?.into_iter().filter(|lane| !matches!(lane.phase, Phase::Landed | Phase::Withdrawn)).map(|lane| lane.branch).collect())
    }
}
