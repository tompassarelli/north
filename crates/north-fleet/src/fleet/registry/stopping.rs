use std::fs;
use std::path::PathBuf;

use crate::error::NorthResult;

use super::super::claims;
use super::super::forget::{self, Forgotten};
use super::super::host::Host;
use super::super::model::Ending;
use super::super::records::{ARCHIVE, write_record};
use super::super::runtime::now_ms;
use super::super::unaccepted::Unaccepted;
use super::super::store::ControlVerb;
use super::{Fleet, session_running};

impl Fleet {
    pub fn stop(&self, name: &str) -> NorthResult<()> {
        let mut record = self.record(name)?;
        let _start = self.start_lock(name)?;
        let host = Host::new(record.host_dir.clone());
        if session_running(&record) {
            let runtime = self.runtime(&record)?;
            let transcript = self.recent(runtime.as_ref(), &record)?;
            let _ = host.send(&runtime.close(&transcript));
        }
        let terminate = !self.host_in_use(&record)?;
        let (was_stopped, was_ended) = (record.stopped, record.ended);
        record.stopped = true;
        record.ended = Some(Ending::Stopped);
        write_record(&self.root.join(name), &record)?;
        if terminate && let Err(error) = host.terminate() {
            record.stopped = was_stopped;
            record.ended = was_ended;
            write_record(&self.root.join(name), &record)?;
            return Err(error);
        }
        if terminate {
            Unaccepted::discard(&record.host_dir);
        }
        claims::release_for(self.claims.as_ref(), &record);
        Ok(())
    }

    /// Forgets `name` here or spools the forget to its host, unless a land-queue ticket on this host
    /// still reports to it; `force` names why it should go anyway.
    pub fn forget_unless_queued(&self, name: &str, force: Option<&str>) -> NorthResult<Forgotten> {
        let cwd = self.session_dir(name)?.exists().then(|| self.record(name)).transpose()?.map(|record| record.cwd);
        let lanes = forget::queued_lanes(&self.state.join("land-queue"), name, cwd.as_deref());
        forget::admit(name, &lanes, force)?;
        let controlled = self.control(ControlVerb::Forget, name)?;
        Ok(Forgotten { controlled, overrode: lanes })
    }

    pub fn forget(&self, name: &str) -> NorthResult<()> {
        self.archive(name)?;
        self.uncache_session(name)
    }

    /// Stops the session and moves its directory out of the registry, returning where it now lies.
    pub(super) fn archive(&self, name: &str) -> NorthResult<PathBuf> {
        self.stop(name)?;
        let archive = self.root.join(ARCHIVE);
        fs::create_dir_all(&archive)?;
        let archived = archive.join(format!("{name}-{}", now_ms()));
        fs::rename(self.root.join(name), &archived)?;
        Ok(archived)
    }
}
