use super::bounded;
use super::failure::{Failure, failed};
use super::follow::own_scope;
use super::remote::OWN_RSH;
use crate::land_offload;
use crate::platform::host;
use crate::platform::records_mirror::{self, ROOT, STAMP, Stamp, TICK_JOURNAL};
use rustix::fs::{FlockOperation, flock};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

const LOCK: &str = "north/records-mirror.lock";
const LAST: &str = "north/records-mirror.last";
const SENT: &str = "north/records-mirror-sent";
const CURSOR: &str = "north/fleet-tick.cursor";
const TICK_UNIT: &str = "fleet-tick.service";
const JOURNAL_FIRST: &str = "--since=-24h";
const JOURNAL_CAP_BYTES: u64 = 8 << 20;
const READ_JOURNAL: Duration = Duration::from_secs(5);
const COPY: Duration = Duration::from_secs(10 * 60);
const RSYNC_PARTIAL: [i32; 2] = [23, 24];

pub fn publish() -> Vec<String> {
    if !land_offload::settles_fleet_here() {
        return Vec::new();
    }
    let mut lines: Vec<String> = fs::read_to_string(records_mirror::state_home().join(LAST)).unwrap_or_default().lines().map(str::to_owned).collect();
    if let Err(error) = std::env::current_exe().and_then(|north| own_scope(north).args(["runtime", "mirror"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0).spawn()) {
        lines.push(format!("records mirror: not started: {error}"));
    }
    lines
}

pub(super) fn run() -> Result<(), Failure> {
    if !land_offload::settles_fleet_here() {
        return Err(failed(1, "this host does not settle the fleet, so it holds no records to copy"));
    }
    let state = records_mirror::state_home();
    let lock = File::create(state.join(LOCK))?;
    if flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err() {
        println!("north runtime mirror: the last copy is still running");
        return Ok(());
    }
    let destinations = land_offload::other_destinations();
    let (copied, failures) = publish_from(&state, &destinations)?;
    fs::write(state.join(LAST), failures.join("\n"))?;
    if !failures.is_empty() {
        return Err(failed(1, failures.join("; ")));
    }
    println!("north runtime mirror: copied {copied} records to {}", destinations.join(", "));
    Ok(())
}

fn publish_from(state: &Path, destinations: &[String]) -> Result<(usize, Vec<String>), Failure> {
    if let Err(why) = export_journal(state) {
        eprintln!("north runtime mirror: fleet tick journal not exported: {why}");
    }
    let stamp = Stamp { host: host::name(), state: state.to_owned(), at: records_mirror::now() };
    fs::write(state.join(STAMP), serde_json::to_vec(&stamp).map_err(|error| failed(1, error.to_string()))?)?;
    let mut current = records_mirror::selected(state, SystemTime::now());
    current.push(PathBuf::from(STAMP));
    fs::create_dir_all(state.join(SENT))?;
    let failures = std::thread::scope(|scope| {
        let copies: Vec<_> = destinations.iter().map(|destination| (destination, scope.spawn(|| copy(state, destination, &current)))).collect();
        copies.into_iter().filter_map(|(destination, copy)| copy.join().unwrap_or_else(|_| Err("the copy panicked".to_owned())).err().map(|why| format!("records mirror: not copied to {destination}: {why}"))).collect()
    });
    Ok((current.len(), failures))
}

fn copy(state: &Path, destination: &str, current: &[PathBuf]) -> Result<(), String> {
    let sent = state.join(SENT).join(destination.replace('/', "_"));
    let kept: BTreeSet<&Path> = current.iter().map(PathBuf::as_path).collect();
    let before = fs::read(&sent).unwrap_or_default();
    let gone = before.split(|byte| *byte == 0).filter(|path| !path.is_empty()).map(|path| Path::new(std::str::from_utf8(path).unwrap_or_default())).filter(|path| !path.as_os_str().is_empty() && !kept.contains(path));
    let listed: Vec<u8> = current.iter().map(PathBuf::as_path).chain(gone).flat_map(|path| path.as_os_str().as_encoded_bytes().iter().copied().chain([0])).collect();
    let list = sent.with_extension("list");
    fs::write(&list, &listed).map_err(|error| error.to_string())?;
    let mut rsync = Command::new("rsync");
    rsync
        .args(["-a", "--copy-dirlinks", "--delete-missing-args", "--from0", "--timeout=20", "-e", OWN_RSH])
        .arg(format!("--files-from={}", list.display()))
        .arg(format!("{}/", state.display()))
        .arg(format!("{destination}:.local/state/{ROOT}/"))
        .stdin(Stdio::null())
        .stdout(Stdio::null());
    let (status, said) = bounded::status_keeping_stderr(&mut rsync, COPY, 4).map_err(|error| error.to_string())?;
    if !status.success() && !status.code().is_some_and(|code| RSYNC_PARTIAL.contains(&code)) {
        return Err(format!("rsync {status}: {}", said.join(" | ")));
    }
    let now: Vec<u8> = current.iter().flat_map(|path| path.as_os_str().as_encoded_bytes().iter().copied().chain([0])).collect();
    fs::write(&sent, now).map_err(|error| error.to_string())
}

fn export_journal(state: &Path) -> Result<(), String> {
    let cursor = state.join(CURSOR);
    let mut journalctl = Command::new("journalctl");
    journalctl.args(["--user", "--unit", TICK_UNIT, "--output=short-iso", "--no-pager", "--quiet"]).stdin(Stdio::null()).stderr(Stdio::null());
    let first = !cursor.is_file();
    match first {
        true => journalctl.args([JOURNAL_FIRST, "--show-cursor"]),
        false => journalctl.arg(format!("--cursor-file={}", cursor.display())),
    };
    let (status, said) = bounded::output(&mut journalctl, READ_JOURNAL).map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("journalctl {status}"));
    }
    let lines = match first {
        true => {
            let (lines, last) = said.trim_end().rsplit_once('\n').unwrap_or(("", said.trim_end()));
            let at = last.strip_prefix("-- cursor: ").ok_or("journalctl named no cursor")?;
            fs::write(&cursor, at).map_err(|error| error.to_string())?;
            if lines.is_empty() { String::new() } else { format!("{lines}\n") }
        }
        false => said,
    };
    let journal = state.join(TICK_JOURNAL);
    if fs::metadata(&journal).is_ok_and(|metadata| metadata.len() > JOURNAL_CAP_BYTES) {
        let text = fs::read(&journal).unwrap_or_default();
        let half = text.len() / 2;
        let start = text[half..].iter().position(|byte| *byte == b'\n').map_or(text.len(), |at| half + at + 1);
        fs::write(&journal, &text[start..]).map_err(|error| error.to_string())?;
    }
    OpenOptions::new().create(true).append(true).open(&journal).and_then(|mut file| file.write_all(lines.as_bytes())).map_err(|error| error.to_string())
}
