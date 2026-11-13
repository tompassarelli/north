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
use super::{Controlled, Fleet, session_running};

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
    /// A worker or subagent was found on the operational reserve.
    Reserve,
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
            Self::Reserve => (Cause::Harness(Harness::Capacity), "stopped on the operational reserve mid-turn"),
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
            phases.stopped(record.launch(), why.label())?;
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
        phases.stopped(record.launch(), why.label())?;
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
        if !self.session_dir(name)?.exists() && self.withdraw_queued(name)? {
            return Ok(Forgotten { controlled: Controlled::Withdrawn, overrode: Vec::new() });
        }
        let record = self.session_dir(name)?.exists().then(|| self.record(name)).transpose()?;
        let lanes = forget::queued_lanes(&self.state.join("land-queue"), name, record.as_ref().map(|record| record.cwd.as_path()));
        let heir = record.as_ref().map(|record| self.lane_heir(record, successor)).transpose()?.flatten();
        forget::admit(name, &lanes, heir.as_deref(), force)?;
        if let (Some(record), Some(_)) = (&record, successor) {
            let _ = self.hand_over_lanes(record, successor, true)?;
        }
        let controlled = self.control(ControlVerb::Forget, name)?;
        Ok(Forgotten { controlled, overrode: if heir.is_some() { Vec::new() } else { lanes } })
    }

    pub fn forget(&self, name: &str) -> NorthResult<()> {
        self.forget_for(name, Halt::Ordered)
    }

    pub(super) fn forget_for(&self, name: &str, why: Halt) -> NorthResult<()> {
        let lane = match self.record(name) {
            Ok(record) if !self.hand_over_lanes(&record, None, false)? => self.remote_lane(&record),
            _ => None,
        };
        self.archive(name, why, why.label())?;
        self.free_remote_lane(name, lane.as_ref());
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
    use crate::fleet::store::{FleetRow, RowKind};

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
    fn a_stop_ordered_on_a_session_whose_own_builds_wait_in_admission_is_held_and_runs_once_they_have_and_one_from_a_sessions_spool_only_for_its_own_child() {
        let home = tempfile::tempdir().unwrap();
        let admission = home.path().join("agent-capacity-v1");
        let fleet = fleet(home.path()).reading_admission(admission.clone());
        let worker = super::super::fixtures::live_under(&fleet, "n757-leases", Some("n757-lead"));
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
        let spooled_by = |session: &str| FleetRow { sender: Sender::Session(session.into()), spooled_by: Some(session.into()), ..fleet.new_row(RowKind::Control, "n757-leases", "stop", &Sender::Tom).unwrap() };
        assert_eq!(fleet.apply_control(&spooled_by("n757-other")).unwrap(), HandedIn::Delivered);
        assert!(!fleet.record("n757-leases").unwrap().ended(), "a stop from another session's spool stopped a session that is not its child");
        fleet.apply_control(&spooled_by("n757-lead")).unwrap();
        assert!(fleet.record("n757-leases").unwrap().ended(), "the stop never ran once the builds had");
    }

    fn recorded(root: &std::path::Path, caller: &str, cwd: &std::path::Path, words: &[String]) -> std::io::Result<()> {
        fs::write(root.join("started"), format!("{caller} in {}: {}", cwd.display(), words.join(" ")))
    }

    #[test]
    fn a_sandboxed_lead_launches_holds_and_forgets_through_the_spool() {
        use std::os::unix::fs::PermissionsExt;
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
        live(&fleet, "n870-lead");
        super::super::fixtures::live_under(&fleet, "n870-child", Some("n870-lead"));
        let spool = fleet.root.join("spool/sessions/n870-lead");
        fs::create_dir_all(&spool).unwrap();
        fs::set_permissions(&fleet.root, fs::Permissions::from_mode(0o555)).unwrap();
        assert!(!fleet.writes_records(), "the fleet root stayed writable, as it does to root, so no sandbox was simulated");
        let lead = Fleet { spools_as: Some("n870-lead".into()), ..fleet.clone() };
        let spooled = |kind: RowKind| -> FleetRow {
            let rows: Vec<FleetRow> = fs::read_dir(&spool).unwrap().flatten().map(|file| serde_json::from_slice(&fs::read(file.path()).unwrap()).unwrap()).filter(|row: &FleetRow| row.kind == kind).collect();
            assert_eq!(rows.len(), 1, "{kind:?}: {rows:?}");
            FleetRow { sender: Sender::Session("n870-lead".into()), spooled_by: Some("n870-lead".into()), ..rows[0].clone() }
        };
        let forgotten = lead.forget_unless_queued("n870-child", None, None).unwrap();
        lead.hold_task("n870-lead", "north#872").unwrap();
        lead.spool_launch("n870-lead", "n870-new", &["--name".into(), "n870-new".into(), "brief".into()]).unwrap();
        fs::set_permissions(&fleet.root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(forgotten.controlled, Controlled::Spooled(_)));
        assert!(fleet.record("n870-child").is_ok(), "the sandbox forgot the session itself");
        assert!(!fleet.record("n870-lead").unwrap().tasks().any(|task| task == "north#872"), "the sandbox wrote the lead's own record");
        let (forget, hold, launch) = (spooled(RowKind::Control), spooled(RowKind::Hold), spooled(RowKind::Launch));
        let tick = Fleet { starts: recorded, ..fleet.clone() };
        let other = |row: &FleetRow| FleetRow { sender: Sender::Session("n870-other".into()), spooled_by: Some("n870-other".into()), ..row.clone() };
        let named = |row: &FleetRow, name: &str| FleetRow { id: format!("{}-again", row.id), text: row.text.replace("n870-new", name), ..row.clone() };
        tick.deliver_row(&other(&forget)).unwrap();
        assert!(fleet.record("n870-child").is_ok(), "a forget from a session that is not the child's parent forgot it");
        assert!(tick.deliver_row(&other(&launch)).is_err());
        assert!(!fleet.root.join("started").exists(), "a launch row that names another session started a session");
        for (id, flag) in [("path-a", "--cwd\",\"/home/.ssh"), ("path-b", "--system-prompt\",\"/home/.ssh/id_ed25519"), ("urgent", "--priority\",\"urgent"), ("aside", "--gates-aside")] {
            let carrying = FleetRow { id: id.into(), text: launch.text.replace("\"brief\"", &format!("\"{flag}\",\"brief\"")), ..launch.clone() };
            tick.deliver_row(&carrying).unwrap();
            assert!(!fleet.root.join("started").exists(), "a spooled launch carrying {flag} started");
        }
        tick.deliver_row(&FleetRow { id: "unspooled".into(), spooled_by: None, ..launch.clone() }).unwrap();
        tick.deliver_row(&FleetRow { id: "unspooled-hold".into(), spooled_by: None, ..hold.clone() }).unwrap();
        assert!(!fleet.root.join("started").exists(), "a launch row no spool carried started a session");
        assert!(!fleet.record("n870-lead").unwrap().tasks().any(|task| task == "north#872"), "a hold row no spool carried took effect");
        tick.deliver_row(&named(&launch, "n870-lead")).unwrap();
        assert!(!fleet.root.join("started").exists(), "a launch took the name of a session that is not the lead's child");
        tick.deliver_row(&forget).unwrap();
        tick.deliver_row(&hold).unwrap();
        tick.deliver_row(&launch).unwrap();
        assert!(fleet.record("n870-child").is_err(), "the lead's forget of its own child never ran");
        assert!(fleet.record("n870-lead").unwrap().tasks().any(|task| task == "north#872"));
        assert_eq!(fs::read_to_string(fleet.root.join("started")).unwrap(), "n870-lead in /tmp: --name n870-new brief");
    }
}
