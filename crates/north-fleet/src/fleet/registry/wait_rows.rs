use std::collections::BTreeSet;
use std::path::Path;

use crate::error::{NorthError, NorthResult};
use crate::fleet::sender::Sender;
use crate::fleet::store::{FleetRow, RowKind, Spool};
use crate::fleet::waits::Run;

use super::{Fleet, HandedIn};

#[derive(Debug, Eq, PartialEq)]
pub enum Waiting {
    Here,
    Spooled { id: String, host: String },
}

impl Fleet {
    pub fn register_wait(&self, session: &str, run: &Run, cwd: &Path) -> NorthResult<Waiting> {
        if self.session_dir(session)?.exists() {
            self.waits().register(session, run.clone(), cwd, None)?;
            return Ok(Waiting::Here);
        }
        let Some(held) = self.held_elsewhere(session)? else { return Err(NorthError::Usage(self.unknown_session(session))) };
        if matches!(run, Run::Capture(_) | Run::Process { .. }) {
            return Err(NorthError::Usage(format!("{run} names a path or process on this machine, which {}, where {session} runs, cannot see; wait on a job, farm or branch run instead", held.host)));
        }
        let row = self.new_row(RowKind::Wait, session, &run.to_string(), &Sender::typed(self.caller()))?;
        Spool::at(&self.root).put(&row)?;
        Ok(Waiting::Spooled { id: row.id, host: held.host })
    }

    pub(super) fn register_wait_row(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        let run: Run = row.text.parse()?;
        self.journaled(row, || {
            let cwd = self.record(&row.to)?.cwd;
            self.waits().register(&row.to, run, &cwd, None)
        })
    }

    fn unknown_session(&self, name: &str) -> String {
        let elsewhere = self.cached_sessions().ok().flatten().unwrap_or_default().into_iter().map(|session| session.host).filter(|host| *host != self.host);
        let hosts: BTreeSet<String> = std::iter::once(self.host.clone()).chain(elsewhere).collect();
        format!("no session named {name} on any host checked ({}); the fleet table lists only live sessions", hosts.into_iter().collect::<Vec<_>>().join(", "))
    }
}

