use std::fs;
use std::path::PathBuf;

use north_base::platform::attribution::{self, Cause, Harness};

use crate::error::NorthResult;

use super::super::claims;
use super::super::forget::{self, Forgotten};
use super::super::host::Host;
use super::super::model::{Ending, Role, Status};
use super::super::records::{ARCHIVE, write_record};
use super::super::runtime::{PROXY, now_ms};
use super::super::unaccepted::Unaccepted;
use super::super::store::ControlVerb;
use super::{Fleet, session_running};

/// Why the fleet stops a session, recorded as the cause of the turn the stop cuts short.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Halt {
    /// `north fleet stop` or `north fleet forget`.
    Ordered,
    Relaunched,
    FailedStart,
    /// Tom opened the conversation in its interactive UI.
    Interactive,
    /// A launch took the session's name over.
    TakenOver,
    Retired,
    /// A proxy-role session ordered the stop, forget or relaunch.
    ByProxy,
}

impl Halt {
    pub(super) fn record(self, name: &str) {
        let (cause, reason) = self.cause();
        attribution::record(name, "fleet", cause, reason);
    }

    fn cause(self) -> (Cause, &'static str) {
        match self {
            Self::Ordered => (Cause::Harness(Harness::Halted), "north fleet stop or forget ended the turn"),
            Self::Relaunched => (Cause::Harness(Harness::Halted), "relaunched under another model or effort mid-turn"),
            Self::FailedStart => (Cause::Harness(Harness::Halted), "stopped after its first turn failed to start"),
            Self::Interactive => (Cause::Tom, "opened in the interactive UI mid-turn"),
            Self::TakenOver => (Cause::Harness(Harness::Halted), "a launch took over its name mid-turn"),
            Self::Retired => (Cause::Harness(Harness::Halted), "retired mid-turn"),
            Self::ByProxy => (Cause::Proxy, "a proxy session stopped it mid-turn"),
        }
    }
}

impl Fleet {
    pub fn stop(&self, name: &str) -> NorthResult<()> {
        self.halt(name, Halt::Ordered)
    }

    /// Stops `name`; a turn the stop cuts short records `why` as its cause.
    pub fn halt(&self, name: &str, why: Halt) -> NorthResult<()> {
        let mut record = self.record(name)?;
        if record.adopted() {
            record.stopped = true;
            record.ended = Some(Ending::Stopped);
            return write_record(&self.root.join(name), &record);
        }
        let _start = self.start_lock(name)?;
        let host = Host::new(record.host_dir.clone());
        let mut mid_turn = false;
        if session_running(&record) {
            let runtime = self.runtime(&record)?;
            let transcript = self.recent(runtime.as_ref(), &record)?;
            mid_turn = transcript.status == Status::Working;
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
        if mid_turn {
            why.record(name);
        }
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
        self.forget_for(name, Halt::Ordered)
    }

    pub(super) fn forget_for(&self, name: &str, why: Halt) -> NorthResult<()> {
        self.archive(name, why)?;
        self.uncache_session(name)
    }

    /// Stops the session and moves its directory out of the registry, returning where it now lies.
    pub(super) fn archive(&self, name: &str, why: Halt) -> NorthResult<PathBuf> {
        self.halt(name, why)?;
        let archive = self.root.join(ARCHIVE);
        fs::create_dir_all(&archive)?;
        let archived = archive.join(format!("{name}-{}", now_ms()));
        fs::rename(self.root.join(name), &archived)?;
        Ok(archived)
    }
}

impl Fleet {
    /// How a stop ordered by `caller` reads: by the proxy when `caller` is a proxy-role session, else `otherwise`.
    pub(super) fn ordered_by(&self, caller: Option<&str>, otherwise: Halt) -> Halt {
        let proxy = caller.is_some_and(|name| name == PROXY || self.record(name).is_ok_and(|record| record.role() == Role::Proxy));
        if proxy { Halt::ByProxy } else { otherwise }
    }
}
