use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Delivery;
use crate::fleet::runtime::{SESSION_ENV, SPOOLED_ENV};
use crate::fleet::sender::Sender;
use crate::fleet::store::{FleetRow, RowKind};

use super::{Fleet, HandedIn};

pub(super) type Starts = fn(&Path, &str, &Path, &[String]) -> io::Result<()>;

const PROBE: &str = ".writable-probe";
const LAUNCH_LOG: &str = ".spooled-launches.log";
const LAUNCH_RUNNING: &str = ".spooled-launch.pid";
const LOG_CAP: u64 = 256 * 1024;
const PATH_FLAGS: [&str; 3] = ["--cwd", "--system-prompt", "--container-dir"];
const CGROUP_FLAGS: [&str; 2] = ["--gates-aside", "--container-shell"];

/// The `fleet launch` flags that take no value: the one list both the launch parser and [`refused_flag`] walk the leading flags by.
pub const BARE_FLAGS: [&str; 7] = ["--test", "--gates-aside", "--container-shell", "--take-over", "--dispatched", "--wait", "--no-wait"];

/// The leading flag of `words` that a spooled launch may not carry, because the host would use a path outside the lead's sandbox, or admit the flag by its own process's cgroup instead of the lead's.
pub fn refused_flag(words: &[String]) -> Option<String> {
    let mut index = 0;
    while let Some(flag) = words.get(index).filter(|word| word.starts_with("--")) {
        if PATH_FLAGS.contains(&flag.as_str()) || CGROUP_FLAGS.contains(&flag.as_str()) {
            return Some(flag.clone());
        }
        if flag == "--priority" && words.get(index + 1).is_some_and(|value| value == "urgent") {
            return Some("--priority urgent".into());
        }
        index += if BARE_FLAGS.contains(&flag.as_str()) { 1 } else { 2 };
    }
    None
}

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

#[derive(Serialize, Deserialize)]
struct SpooledLaunch {
    name: String,
    words: Vec<String>,
}

impl Fleet {
    /// Whether this process may write the fleet's session records: a worker sandbox reads them but may not write them, so its launch, hold and forget wait in the spool for this host's fleet tick.
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

    /// Spools `words`, the arguments of a `north fleet launch` of session `name` by `caller`, for this host's fleet tick to run as that caller, so every admission a direct launch runs judges it there.
    pub fn spool_launch(&self, caller: &str, name: &str, words: &[String]) -> NorthResult<FleetRow> {
        let text = serde_json::to_string(&SpooledLaunch { name: name.into(), words: words.to_vec() })?;
        let row = self.new_row(RowKind::Launch, caller, &text, &Sender::Session(caller.into()))?;
        self.put_spooled("launch", &row)?;
        Ok(row)
    }

    pub(super) fn apply_hold(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        let (verb, task) = row.text.split_once(' ').ok_or_else(|| NorthError::Usage(format!("HOLD row {} names no task", row.id)))?;
        if row.spooled_by.as_deref() != Some(row.to.as_str()) {
            eprintln!("north fleet: refused HOLD row {}: only the spool of the session it holds for may carry it", row.id);
            return self.journaled(row, || Ok(()));
        }
        self.journaled(row, || match verb {
            "hold" => self.hold_task_here(&row.to, task),
            "release" => self.release_task_here(&row.to, task),
            other => Err(NorthError::Usage(format!("HOLD row {} asks for {other}, not hold or release", row.id))),
        })
    }

    pub(super) fn apply_launch(&self, row: &FleetRow) -> NorthResult<HandedIn> {
        let launch: SpooledLaunch = serde_json::from_str(&row.text)?;
        let writer = row.sender.session().filter(|writer| *writer == row.to).ok_or_else(|| NorthError::Usage(format!("LAUNCH row {} names no session that asks for it", row.id)))?;
        if row.spooled_by.as_deref() != Some(writer) {
            eprintln!("north fleet: refused LAUNCH row {}: only the spool of the session it launches for may carry it", row.id);
            return self.journaled(row, || Ok(()));
        }
        self.journaled(row, || {
            if let Some(flag) = refused_flag(&launch.words) {
                let refusal = format!("north fleet launch {}: refused, a launch from your sandbox cannot carry {flag}: the host would use or judge it outside your sandbox", launch.name);
                eprintln!("{refusal}");
                self.tell(writer, &refusal, Delivery::Queued);
                return Ok(());
            }
            if self.record(&launch.name).is_ok_and(|held| held.parent.as_deref() != Some(writer)) {
                let refusal = format!("north fleet launch {}: refused, a session of that name exists and is not yours", launch.name);
                eprintln!("{refusal}");
                self.tell(writer, &refusal, Delivery::Queued);
                return Ok(());
            }
            let cwd = self.record(writer)?.cwd;
            match (self.starts)(&self.root, writer, &cwd, &launch.words) {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => Err(NorthError::State(format!("north fleet launch {} for {writer} waits for the next tick: an earlier spooled launch is still running", launch.name))),
                started => started.map_err(|error| NorthError::State(format!("north fleet launch {} for {writer}: could not start it, logging to {}: {error}", launch.name, self.root.join(LAUNCH_LOG).display()))),
            }
        })
    }
}

/// Starts `north fleet launch WORDS` as `caller` in `cwd` and leaves it running, one at a time: while an earlier one lives it answers `WouldBlock`, so a lead's spooled launches never race each other's claim and hold checks.
pub(super) fn start_launch(root: &Path, caller: &str, cwd: &Path, words: &[String]) -> io::Result<()> {
    let running = root.join(LAUNCH_RUNNING);
    if fs::read_to_string(&running).ok().and_then(|pid| pid.trim().parse::<u32>().ok()).is_some_and(|pid| Path::new("/proc").join(pid.to_string()).exists()) {
        return Err(io::ErrorKind::WouldBlock.into());
    }
    let log = OpenOptions::new().create(true).append(true).open(root.join(LAUNCH_LOG))?;
    if log.metadata()?.len() > LOG_CAP {
        log.set_len(0)?;
    }
    let exe = std::env::current_exe()?;
    let north: PathBuf = exe.to_str().and_then(|path| path.strip_suffix(" (deleted)")).map_or(exe.clone(), PathBuf::from);
    let mut child = Command::new(north).args(["fleet", "launch"]).args(words).current_dir(cwd).env(SESSION_ENV, caller).env(SPOOLED_ENV, "1").stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log).process_group(0).spawn()?;
    let _ = fs::write(&running, child.id().to_string());
    std::thread::spawn(move || child.wait());
    Ok(())
}
