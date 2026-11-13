use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Delivery;
use crate::fleet::runtime::{SESSION_ENV, SPOOLED_ENV, binary, new_uuid};
use crate::fleet::sender::Sender;
use crate::fleet::store::{FleetRow, RowKind};

use super::{Fleet, HandedIn};

pub(super) type Starts = fn(&Path, &str, &Path, &[String]) -> io::Result<()>;

const PROBE: &str = ".writable-probe";
const LAUNCH_LOG: &str = ".spooled-launches.log";
const LAUNCH_RUNNING: &str = ".spooled-launch.pid";
const LOG_CAP: u64 = 256 * 1024;

#[derive(Clone, Copy)]
pub(super) enum Holding {
    Take,
    Drop,
}

impl Holding {
    fn word(self) -> &'static str {
        match self {
            Self::Take => "hold",
            Self::Drop => "release",
        }
    }
}

/// The words of a lead's spooled `north fleet launch` or `north fleet revive`, after the verb.
#[derive(Serialize, Deserialize)]
struct SpooledVerb {
    words: Vec<String>,
}

impl Fleet {
    /// Whether this process may write the fleet's session records: a worker sandbox reads them but may not write them, so its launch, revive, hold and forget wait in the spool for this host's fleet tick.
    pub fn writes_records(&self) -> bool {
        let probe = self.root.join(format!("{PROBE}-{}", std::process::id()));
        match OpenOptions::new().write(true).create_new(true).open(&probe) {
            Ok(_) => {
                let _ = fs::remove_file(&probe);
                true
            }
            Err(error) => error.kind() != io::ErrorKind::PermissionDenied,
        }
    }

    pub(super) fn put_spooled(&self, verb: &str, row: &FleetRow) -> NorthResult<()> {
        self.spool().put(row).map_err(|error| NorthError::Refused(format!("north fleet {verb}: could not spool its row to {}: {}", self.spool().location().display(), error.user_message())))
    }

    pub(super) fn spool_hold(&self, session: &str, task: &str, holding: Holding) -> NorthResult<FleetRow> {
        let row = self.new_row(RowKind::Hold, session, &format!("{} {task}", holding.word()), &Sender::Session(session.into()))?;
        self.put_spooled(holding.word(), &row)?;
        Ok(row)
    }

    /// Spools `words`, the arguments after the verb of a `north fleet launch` (`kind` LAUNCH) or `north fleet revive` (`kind` REVIVE) by `caller`, for this host's fleet tick to run as the lead-only grant of `lead_grant.rs` allows.
    pub fn spool_verb(&self, kind: RowKind, caller: &str, words: &[String]) -> NorthResult<FleetRow> {
        let text = serde_json::to_string(&SpooledVerb { words: words.to_vec() })?;
        let row = self.new_row(kind, caller, &text, &Sender::Session(caller.into()))?;
        self.put_spooled(&kind.label().to_ascii_lowercase(), &row)?;
        Ok(row)
    }

    /// Refuses the spooled row `row` of `writer` with `why`: the tick logs it, tells the writer and journals the row, so it never runs.
    fn refuse_spooled(&self, row: &FleetRow, writer: &str, why: &str) -> NorthResult<HandedIn> {
        let refusal = format!("north fleet {} row {}: refused, {why}", row.kind.label(), row.id);
        self.journaled(row, || {
            eprintln!("{refusal}");
            self.tell(writer, &refusal, Delivery::Queued);
            Ok(())
        })
    }

    pub(super) fn apply_hold(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        let (verb, task) = row.text.split_once(' ').ok_or_else(|| NorthError::Usage(format!("HOLD row {} names no task", row.id)))?;
        if row.spooled_by.as_deref() != Some(row.to.as_str()) {
            eprintln!("north fleet: refused HOLD row {}: only the spool of the session it holds for may carry it", row.id);
            return self.journaled(row, || Ok(()));
        }
        if let Some(why) = self.refused_hold(&row.to, task) {
            return self.refuse_spooled(row, &row.to, &why);
        }
        self.journaled(row, || match verb {
            "hold" => self.hold_task_here(&row.to, task),
            "release" => self.release_task_here(&row.to, task),
            other => Err(NorthError::Usage(format!("HOLD row {} asks for {other}, not hold or release", row.id))),
        })
    }

    /// Runs a lead's spooled launch or revive as the lead, in its own project's main checkout, with only the words its grant allows (lead_grant.rs).
    pub(super) fn apply_spooled_verb(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        let spooled: SpooledVerb = serde_json::from_str(&row.text)?;
        let kind = row.kind.label();
        let writer = row.sender.session().filter(|writer| *writer == row.to).ok_or_else(|| NorthError::Usage(format!("{kind} row {} names no session that asks for it", row.id)))?;
        if row.spooled_by.as_deref() != Some(writer) {
            eprintln!("north fleet: refused {kind} row {}: only the spool of the session that asks for it may carry it", row.id);
            return self.journaled(row, || Ok(()));
        }
        let granted = match row.kind {
            RowKind::Launch => self.granted_launch(writer, &spooled.words),
            RowKind::Revive => self.granted_revive(writer, &spooled.words),
            other => return Err(NorthError::Usage(format!("a {} row is no spooled fleet verb", other.label()))),
        };
        let granted = match granted {
            Ok(granted) => granted,
            Err(why) => return self.refuse_spooled(row, writer, &why),
        };
        if row.kind == RowKind::Launch && self.record(&granted.name).is_ok_and(|held| held.parent.as_deref() != Some(writer)) {
            return self.refuse_spooled(row, writer, &format!("a session named {} exists and is not yours", granted.name));
        }
        self.journaled(row, || match (self.starts)(&self.root, writer, &granted.cwd, &granted.arguments) {
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => Err(NorthError::State(format!("north fleet {} {} for {writer} waits for the next tick: an earlier spooled verb is still running", granted.arguments[0], granted.name))),
            started => started.map_err(|error| NorthError::State(format!("north fleet {} {} for {writer}: could not start it, logging to {}: {error}", granted.arguments[0], granted.name, self.root.join(LAUNCH_LOG).display()))),
        })
    }
}

/// Starts `north fleet ARGUMENTS` as `caller` in `cwd` and leaves it running, one at a time: while an earlier one lives it answers `WouldBlock`, so a lead's spooled launches and revives never race each other's claim and hold checks.
pub(super) fn start_spooled(root: &Path, caller: &str, cwd: &Path, arguments: &[String]) -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let north: PathBuf = exe.to_str().and_then(|path| path.strip_suffix(" (deleted)")).map_or(exe.clone(), PathBuf::from);
    start_scoped(root, caller, cwd, arguments, &binary("systemd-run"), &north)
}

/// The start of [`start_spooled`] in a transient scope of its own through `systemd_run`: the tick runs as a oneshot unit whose stop kills every process left in its cgroup, so a verb started there died between starting a session host and sending its brief (north#1054).
pub(super) fn start_scoped(root: &Path, caller: &str, cwd: &Path, arguments: &[String], systemd_run: &Path, north: &Path) -> io::Result<()> {
    let running = root.join(LAUNCH_RUNNING);
    if fs::read_to_string(&running).ok().and_then(|pid| pid.trim().parse::<u32>().ok()).is_some_and(|pid| Path::new("/proc").join(pid.to_string()).exists()) {
        return Err(io::ErrorKind::WouldBlock.into());
    }
    let log = OpenOptions::new().create(true).append(true).open(root.join(LAUNCH_LOG))?;
    if log.metadata()?.len() > LOG_CAP {
        log.set_len(0)?;
    }
    let unit = format!("north-spooled-{}", new_uuid().map_err(|error| io::Error::other(error.user_message()))?);
    let mut child = Command::new(systemd_run).args(["--user", "--scope", "--quiet", "--collect", "--property=RuntimeMaxSec=600", "--unit", &unit, "--"]).arg(north).arg("fleet").args(arguments).current_dir(cwd).env(SESSION_ENV, caller).env(SPOOLED_ENV, "1").stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log).process_group(0).spawn()?;
    let _ = fs::write(&running, child.id().to_string());
    std::thread::spawn(move || child.wait());
    Ok(())
}
