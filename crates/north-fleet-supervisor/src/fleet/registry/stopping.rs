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
        if let (Controlled::Here, Some(_), Some(heir)) = (&controlled, successor, heir.as_deref()) {
            self.move_live_children(name, heir)?;
        }
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
        super::super::fixtures::lead(&fleet, "n757-lead", "north#757");
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

    fn scoped(root: &std::path::Path, caller: &str, cwd: &std::path::Path, words: &[String]) -> std::io::Result<()> {
        let bin = root.join(".bin");
        super::super::spooled::start_scoped(root, caller, cwd, words, &bin.join("systemd-run"), &bin.join("north"))
    }

    struct SpooledLead {
        fleet: Fleet,
        tick: Fleet,
        forget: FleetRow,
        hold: FleetRow,
        launch: FleetRow,
        revive: FleetRow,
    }

    fn words(text: &str) -> Vec<String> {
        text.split(' ').map(str::to_owned).collect()
    }

    fn asking(row: &FleetRow, id: &str, text: &str) -> FleetRow {
        FleetRow { id: id.into(), text: serde_json::json!({"words": words(text)}).to_string(), ..row.clone() }
    }

    fn recorded_start(fleet: &Fleet) -> Option<String> {
        fs::read_to_string(fleet.root.join("started")).ok()
    }

    /// A lead in a sandbox that may not write the fleet's records, whose forget, hold, launch and revive wait in its spool, and a tick that records what it starts.
    fn spooled_lead(home: &std::path::Path) -> SpooledLead {
        use std::os::unix::fs::PermissionsExt;
        use crate::fleet::claims::Rows;
        let fleet = fleet(home);
        live(&fleet, "n870-a");
        fleet.claims.hold("north#870@the first box", "n870-a", &Default::default()).unwrap();
        fleet.claims.hold("north#870@the second box", "n870-a", &Default::default()).unwrap();
        fleet.claims.hold("north#871@another box", "n871-b", &Default::default()).unwrap();
        fleet.forget("n870-a").unwrap();
        let items = ["north#870".to_owned(), "north#871".to_owned()];
        assert_eq!(fleet.claims.holders(Rows::Items(&items)).unwrap(), [("north#871@another box".to_owned(), "n871-b".to_owned())]);
        super::super::fixtures::lead(&fleet, "n870-lead", "tompassarelli/north#870");
        fleet.hold_task("n870-lead", "makgora#1").unwrap();
        super::super::fixtures::live_under(&fleet, "n870-child", Some("n870-lead"));
        let stopped = fleet.root.join("n870-stopped");
        fs::create_dir_all(&stopped).unwrap();
        fs::write(stopped.join("session.json"), serde_json::json!({"name": "n870-stopped", "provider": "claude", "account": "a", "cwd": "/tmp", "model": null, "created_ms": 1, "host_pid": null, "conversation": "n870-stopped", "parent": "n870-lead", "host_dir": stopped, "stream_offset": 0}).to_string()).unwrap();
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
        lead.spool_verb(RowKind::Launch, "n870-lead", &words("--name n870-new --task north#870 brief")).unwrap();
        lead.spool_verb(RowKind::Revive, "n870-lead", &words("n870-stopped --effort high")).unwrap();
        fs::set_permissions(&fleet.root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(forgotten.controlled, Controlled::Spooled(_)));
        assert!(fleet.record("n870-child").is_ok(), "the sandbox forgot the session itself");
        assert!(!fleet.record("n870-lead").unwrap().tasks().any(|task| task == "north#872"), "the sandbox wrote the lead's own record");
        let (forget, hold, launch, revive) = (spooled(RowKind::Control), spooled(RowKind::Hold), spooled(RowKind::Launch), spooled(RowKind::Revive));
        let tick = Fleet { starts: recorded, ..fleet.clone() };
        SpooledLead { fleet, tick, forget, hold, launch, revive }
    }

    #[test]
    fn a_worker_role_writer_spooling_its_verbs_is_refused_by_the_tick_and_so_is_a_lead_asking_outside_its_grant() {
        let tempdir = tempfile::tempdir().unwrap();
        let home = tempdir.path().canonicalize().unwrap();
        let SpooledLead { fleet, tick, forget, hold, launch, revive } = spooled_lead(&home);
        let started = || recorded_start(&fleet);
        let from = |row: &FleetRow, writer: &str| FleetRow { id: format!("{}-{writer}", row.id), to: writer.into(), sender: Sender::Session(writer.into()), spooled_by: Some(writer.into()), ..row.clone() };
        tick.deliver_row(&FleetRow { sender: Sender::Session("n870-other".into()), spooled_by: Some("n870-other".into()), ..forget.clone() }).unwrap();
        assert!(fleet.record("n870-child").is_ok(), "a forget from a session that is not the child's parent forgot it");
        assert!(tick.deliver_row(&FleetRow { sender: Sender::Session("n870-other".into()), spooled_by: Some("n870-other".into()), ..launch.clone() }).is_err());
        for row in [from(&launch, "n870-child"), from(&revive, "n870-child"), from(&hold, "n870-child"), from(&forget, "n870-child")] {
            tick.deliver_row(&row).unwrap();
        }
        assert_eq!(started(), None, "a worker's spooled launch or revive started a session");
        assert!(fleet.record("n870-child").is_ok() && !fleet.record("n870-child").unwrap().tasks().any(|task| task == "north#872"), "a worker's spooled hold or forget took effect");
        let main = home.join("code/north/main");
        fs::create_dir_all(home.join("code/north/worktrees/n870-lane")).unwrap();
        fs::create_dir_all(home.join("code/other/main")).unwrap();
        std::os::unix::fs::symlink(&main, home.join("main-link")).unwrap();
        let (main_text, home_text) = (main.display().to_string(), home.display().to_string());
        let refused = [
            "--parent proxy", "--parent n870-lead", "--role lead", "--role worker", "--role proxy", "--account a", "--system-prompt /etc/passwd", "--prompt vanilla",
            "--container-shell", "--container-dir /work", "--take-over", "--dispatched", "--test", "--gates-aside", "--priority urgent", "--local why", "--pin why",
            "--foreground-cap 10", "--provider opencode", "--route api:org", "--model api:key", "--route claude-opus,api", "--model SK-abc", "--effort huge", "--idle-exit 1h",
            "--task north#999", "--task makgora#1", "--name n870-again",
        ];
        let checkouts = [format!("{home_text}/code/north/worktrees/n870-lane"), format!("{home_text}/code/other/main"), format!("{main_text}/../main"), format!("{home_text}/main-link")];
        for (index, extra) in refused.iter().map(|flag| (*flag).to_owned()).chain(checkouts.iter().map(|path| format!("--cwd {path}"))).enumerate() {
            tick.deliver_row(&asking(&launch, &format!("refused-{index}"), &format!("--name n870-new --task north#870 {extra} brief"))).unwrap();
            assert_eq!(started(), None, "a spooled launch carrying {extra} started");
        }
        for (id, text) in [("no-task", "--name n870-new brief"), ("no-brief", "--name n870-new --task north#870"), ("bad-name", "--name ../n870 --task north#870 brief")] {
            tick.deliver_row(&asking(&launch, id, text)).unwrap();
            assert_eq!(started(), None, "the spooled launch {text} started");
        }
        for (id, text) in [("live", "n870-child"), ("not-a-child", "n870-lead"), ("no-session", "n870-none"), ("climbs", "../n870-stopped"), ("prompt", "n870-stopped --system-prompt /etc/passwd"), ("account", "n870-stopped --account a"), ("effort", "n870-stopped --effort huge")] {
            tick.deliver_row(&asking(&revive, id, text)).unwrap();
            assert_eq!(started(), None, "the spooled revive {text} started");
        }
        tick.deliver_row(&FleetRow { id: "other-project-hold".into(), text: "hold makgora#2".into(), ..hold.clone() }).unwrap();
        assert!(!fleet.record("n870-lead").unwrap().tasks().any(|task| task == "makgora#2"), "a lead's spooled hold took an issue of another project");
        tick.deliver_row(&FleetRow { id: "unspooled".into(), spooled_by: None, ..launch.clone() }).unwrap();
        tick.deliver_row(&FleetRow { id: "unspooled-hold".into(), spooled_by: None, ..hold }).unwrap();
        assert_eq!(started(), None, "a launch row no spool carried started a session");
        assert!(!fleet.record("n870-lead").unwrap().tasks().any(|task| task == "north#872"), "a hold row no spool carried took effect");
        tick.deliver_row(&FleetRow { id: "taken".into(), text: launch.text.replace("n870-new", "n870-lead"), ..launch.clone() }).unwrap();
        assert_eq!(started(), None, "a launch took the name of a session that is not the lead's child");
    }

    #[test]
    fn a_sandboxed_lead_launches_holds_and_forgets_through_the_spool_and_its_launch_outlives_the_tick_that_started_it() {
        let tempdir = tempfile::tempdir().unwrap();
        let home = tempdir.path().canonicalize().unwrap();
        let SpooledLead { fleet, tick, forget, hold, launch, revive } = spooled_lead(&home);
        let started = || recorded_start(&fleet);
        let main_text = home.join("code/north/main").display().to_string();
        tick.deliver_row(&forget).unwrap();
        tick.deliver_row(&hold).unwrap();
        tick.deliver_row(&launch).unwrap();
        assert!(fleet.record("n870-child").is_err(), "the lead's forget of its own child never ran");
        assert!(fleet.record("n870-lead").unwrap().tasks().any(|task| task == "north#872"));
        assert_eq!(started().unwrap(), format!("n870-lead in {main_text}: launch --provider claude --role worker --parent n870-lead --name n870-new --task north#870 --cwd {main_text} brief"));
        tick.deliver_row(&asking(&launch, "own-cwd", &format!("--name n870-new --task north#870 --cwd {main_text} --no-wait brief"))).unwrap();
        assert_eq!(started().unwrap(), format!("n870-lead in {main_text}: launch --provider claude --role worker --parent n870-lead --name n870-new --task north#870 --no-wait --cwd {main_text} brief"));
        tick.deliver_row(&revive).unwrap();
        assert_eq!(started().unwrap(), format!("n870-lead in {main_text}: revive n870-stopped --effort high"));

        use std::os::unix::fs::PermissionsExt;
        let bin = fleet.root.join(".bin");
        fs::create_dir_all(&bin).unwrap();
        for (program, body) in [("systemd-run", "case \" $* \" in *' --user --scope '*) echo $$ > \"$0.scoped\";; esac\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n"), ("north", "echo host > \"$0.trace\"\nwhile [ ! -e \"$0.tick-ended\" ]; do sleep 0.05; done\necho brief >> \"$0.trace\"\n")] {
            fs::write(bin.join(program), format!("#!/bin/sh\n{body}")).unwrap();
            fs::set_permissions(bin.join(program), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let trace = || fs::read_to_string(bin.join("north.trace")).unwrap_or_default();
        Fleet { starts: scoped, ..fleet.clone() }.deliver_row(&asking(&launch, "outlives-tick", "--name n870-new --task north#870 brief")).unwrap();
        assert!(north_base::platform::wait::until(std::time::Duration::from_secs(10), || trace() == "host\n"), "the spooled launch never started its host: {:?}", trace());
        let launch_pid = fs::read_to_string(fleet.root.join(".spooled-launch.pid")).unwrap();
        if fs::read_to_string(bin.join("systemd-run.scoped")).ok() != Some(format!("{launch_pid}\n")) {
            std::process::Command::new("kill").args(["-KILL", &launch_pid]).status().unwrap();
        }
        fs::write(bin.join("north.tick-ended"), "").unwrap();
        assert!(north_base::platform::wait::until(std::time::Duration::from_secs(10), || trace() == "host\nbrief\n"), "the tick's end, which kills every process left in its unit, killed the spooled launch between its host and its brief: {:?}", trace());
    }
}
