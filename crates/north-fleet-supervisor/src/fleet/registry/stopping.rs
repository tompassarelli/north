use std::fs;
use std::path::PathBuf;

use north_base::platform::attribution::{self, Cause, Harness};

use crate::error::NorthResult;

use super::super::builds;
use super::super::claims;
use super::super::forget::{self, Forgotten};
use super::super::host::Host;
use super::super::model::{Phase, Role, Status};
use super::super::records::{ARCHIVE, read_record};
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
    MovedAccount,
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

    /// What stopped the session, as its lifecycle record says.
    pub(super) fn label(self) -> &'static str {
        self.cause().1.trim_end_matches(" mid-turn")
    }

    fn cause(self) -> (Cause, &'static str) {
        match self {
            Self::Ordered => (Cause::Harness(Harness::Halted), "north fleet stop or forget ended the turn"),
            Self::Relaunched => (Cause::Harness(Harness::Halted), "relaunched under another model or effort mid-turn"),
            Self::MovedAccount => (Cause::Harness(Harness::Capacity), "moved onto another account mid-turn"),
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

    /// Why a stop ordered on `name` waits: a build it started is still queued or running in capacity admission, and its result wakes the session. `forget` ends the session regardless.
    pub(super) fn stop_refusal(&self, name: &str) -> NorthResult<Option<String>> {
        if self.record(name)?.ended() {
            return Ok(None);
        }
        let held = builds::held(&self.admission, &self.proc, name);
        let listed: Vec<&str> = held.iter().map(|standing| standing.label()).collect();
        Ok((!held.is_empty()).then(|| format!("fleet stop {name}: its own builds are still in capacity admission ({}), and their result wakes it; stop it once they have run, or forget it", listed.join(", "))))
    }

    /// Stops `name`; a turn the stop cuts short records `why` as its cause.
    pub fn halt(&self, name: &str, why: Halt) -> NorthResult<()> {
        self.halt_cut_short(name, why).map(drop)
    }

    /// Stops `name` as `halt` does and says whether the stop cut a running turn short.
    pub(super) fn halt_cut_short(&self, name: &str, why: Halt) -> NorthResult<bool> {
        let record = self.record(name)?;
        let mut phases = self.phases()?;
        if record.adopted() {
            phases.moved(record.launch(), Phase::Stopped, why.label())?;
            return Ok(false);
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
        phases.moved(record.launch(), Phase::Stopped, why.label())?;
        if terminate && let Err(error) = host.terminate() {
            if record.live() {
                phases.live(record.launch(), record.host_pid, &format!("its host did not stop: {}", error.user_message()))?;
            }
            return Err(error);
        }
        if terminate {
            Unaccepted::discard(&record.host_dir);
        }
        claims::release_for(self.claims.as_ref(), &record);
        if mid_turn {
            why.record(name);
        }
        Ok(mid_turn)
    }

    /// Forgets `name` here or spools the forget to its host, unless a land-queue ticket on this host
    /// still reports to it; `force` names why it should go anyway.
    pub fn forget_unless_queued(&self, name: &str, force: Option<&str>, successor: Option<&str>) -> NorthResult<Forgotten> {
        let record = self.session_dir(name)?.exists().then(|| self.record(name)).transpose()?;
        let lanes = forget::queued_lanes(&self.state.join("land-queue"), name, record.as_ref().map(|record| record.cwd.as_path()));
        let heir = record.as_ref().map(|record| self.lane_heir(record, successor)).transpose()?.flatten();
        forget::admit(name, &lanes, heir.as_deref(), force)?;
        if let (Some(record), Some(_)) = (&record, successor) {
            self.hand_over_lanes(record, successor, true)?;
        }
        let controlled = self.control(ControlVerb::Forget, name)?;
        Ok(Forgotten { controlled, overrode: if heir.is_some() { Vec::new() } else { lanes } })
    }

    pub fn forget(&self, name: &str) -> NorthResult<()> {
        self.forget_for(name, Halt::Ordered)
    }

    pub(super) fn forget_for(&self, name: &str, why: Halt) -> NorthResult<()> {
        if let Ok(record) = self.record(name) {
            self.hand_over_lanes(&record, None, false)?;
        }
        self.archive(name, why, why.label())?;
        self.release_all_claims_of(name)?;
        self.uncache_session(name)
    }

    /// Stops the session, moves its directory out of the registry and retires its launch for `cause`, returning
    /// where it now lies.
    pub(super) fn archive(&self, name: &str, why: Halt, cause: &str) -> NorthResult<PathBuf> {
        self.halt(name, why)?;
        let archive = self.root.join(ARCHIVE);
        fs::create_dir_all(&archive)?;
        let archived = archive.join(format!("{name}-{}", now_ms()));
        fs::rename(self.root.join(name), &archived)?;
        self.phases()?.moved(read_record(&archived)?.launch(), Phase::Retired, cause)?;
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

#[cfg(test)]
mod tests {
    use super::super::fixtures::{fleet, live};
    use super::super::HandedIn;
    use super::*;
    use crate::error::NorthError;
    use crate::fleet::sender::Sender;
    use crate::fleet::store::RowKind;

    const REPORT: &str = "Not done: Clippy and the lease_lifecycle test run are queued in admission; I will finish when they report.\nStop: capacity";

    fn started(pid: u32) -> String {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
        stat[stat.rfind(')').unwrap() + 2..].split(' ').nth(19).unwrap().to_owned()
    }

    fn build(admission: &std::path::Path, directory: &str, file: &str, owner: &str) -> PathBuf {
        fs::create_dir_all(admission.join(directory)).unwrap();
        let path = admission.join(directory).join(file);
        let entry = serde_json::json!({"owner": owner, "wrapperPid": std::process::id(), "wrapperStart": started(std::process::id())});
        fs::write(&path, entry.to_string()).unwrap();
        path
    }

    #[test]
    fn a_stop_ordered_on_a_session_whose_own_builds_wait_in_admission_is_held_and_runs_once_they_have() {
        let home = tempfile::tempdir().unwrap();
        let admission = home.path().join("agent-capacity-v1");
        let fleet = fleet(home.path()).reading_admission(admission.clone());
        let worker = live(&fleet, "n757-leases");
        let stream = serde_json::json!({"type": "assistant", "session_id": "n757-leases", "message": {"content": [{"type": "text", "text": REPORT}]}});
        fs::write(fleet.root.join("n757-leases/g1/stream.jsonl"), format!("{stream}\n")).unwrap();
        let clippy = build(&admission, "queue", "001-clippy.json", "gate:n757-leases");
        let tests = build(&admission, "leases", "lease-tests.json", "gate:n757-leases");
        build(&admission, "queue", "002-other.json", "gate:another-session");

        let Err(NorthError::Refused(why)) = fleet.run_control(ControlVerb::Stop, "n757-leases", None) else { panic!("a stop cut off the builds its session waits for") };
        let row = fleet.new_row(RowKind::Control, "n757-leases", "stop", &Sender::Tom).unwrap();

        assert!(why.contains("queued in admission, running under an admission lease"), "{why}");
        assert_eq!(fleet.apply_control(&row).unwrap(), HandedIn::Delivered);
        assert!(!fleet.record("n757-leases").unwrap().ended(), "a stop row held the session too");
        assert!(worker.frames.lock().unwrap().is_empty(), "the held stop still closed the session's turn");

        fs::remove_file(clippy).unwrap();
        fs::remove_file(tests).unwrap();
        fleet.run_control(ControlVerb::Stop, "n757-leases", None).unwrap();
        assert!(fleet.record("n757-leases").unwrap().ended(), "the stop never ran once the builds had");
    }

    #[test]
    fn a_forgotten_session_holds_no_claim_in_the_ledger_including_boxes_it_claimed_itself() {
        use crate::fleet::claims::Rows;
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        live(&fleet, "n870-a");
        fleet.claims.hold("north#870@the first box", "n870-a", &Default::default()).unwrap();
        fleet.claims.hold("north#870@the second box", "n870-a", &Default::default()).unwrap();
        fleet.claims.hold("north#871@another box", "n871-b", &Default::default()).unwrap();

        fleet.forget("n870-a").unwrap();

        let items = ["north#870".to_owned(), "north#871".to_owned()];
        assert_eq!(fleet.claims.holders(Rows::Items(&items)).unwrap(), [("north#871@another box".to_owned(), "n871-b".to_owned())]);
    }
}
