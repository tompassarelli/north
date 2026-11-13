use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::fs::{FlockOperation, flock};

pub const RETRY: i32 = north_base::foundation::exit::NO_VERDICT_STATUS;
const POLL: Duration = Duration::from_millis(50);
const COMMAND_CHARS: usize = 200;
const USER_HZ: u64 = 100;

pub struct IngestLock {
    _file: File,
}

pub fn acquire(path: &Path, wait: Duration) -> io::Result<Option<IngestLock>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).truncate(false).write(true).open(path)?;
    let deadline = Instant::now() + wait;
    loop {
        match flock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => {
                file.set_len(0)?;
                writeln!(file, "{} {}", std::process::id(), now_ms())?;
                return Ok(Some(IngestLock { _file: file }));
            }
            Err(rustix::io::Errno::WOULDBLOCK) if Instant::now() < deadline => sleep(POLL),
            Err(rustix::io::Errno::WOULDBLOCK) => return Ok(None),
            Err(error) => return Err(error.into()),
        }
    }
}

pub struct Holder {
    pub pid: u32,
    pub command: String,
    pub held: Duration,
}

impl fmt::Display for Holder {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "pid {} `{}`, holding it for {} s", self.pid, self.command, self.held.as_secs())
    }
}

pub fn holder(path: &Path) -> Option<Holder> {
    holder_in(path, Path::new("/proc"))
}

fn holder_in(path: &Path, proc_root: &Path) -> Option<Holder> {
    let meta = fs::metadata(path).ok()?;
    let device = format!("{:02x}:{:02x}:{}", rustix::fs::major(meta.dev()), rustix::fs::minor(meta.dev()), meta.ino());
    let inode = format!(":{}", meta.ino());
    let locks = fs::read_to_string(proc_root.join("locks")).ok()?;
    let held: Vec<(u32, bool)> = locks
        .lines()
        .filter_map(|line| match line.split_whitespace().collect::<Vec<_>>().as_slice() {
            [_, "FLOCK", _, _, pid, file, ..] if file.ends_with(&inode) => Some((pid.parse().ok()?, *file == device)),
            _ => None,
        })
        .collect();
    let (pid, _) = held.iter().find(|(_, exact)| *exact).or_else(|| (held.len() == 1).then(|| &held[0]))?;
    let process = proc_root.join(pid.to_string());
    let cmdline = fs::read(process.join("cmdline")).ok()?;
    let mut command = cmdline.split(|byte| *byte == 0).filter(|word| !word.is_empty()).map(String::from_utf8_lossy).collect::<Vec<_>>().join(" ");
    if let Some((end, _)) = command.char_indices().nth(COMMAND_CHARS) {
        command.truncate(end);
    }
    let recorded = fs::read_to_string(path).ok().and_then(|text| {
        let (owner, since) = text.trim().split_once(' ')?;
        (owner.parse::<u32>().ok()? == *pid).then(|| since.parse::<u64>().ok()).flatten()
    });
    let since_ms = recorded.or_else(|| started_ms(&process, proc_root))?;
    Some(Holder { pid: *pid, command, held: Duration::from_millis(now_ms().saturating_sub(since_ms)) })
}

fn started_ms(process: &Path, proc_root: &Path) -> Option<u64> {
    let ticks: u64 = fs::read_to_string(process.join("stat")).ok()?.rsplit_once(')')?.1.split_whitespace().nth(19)?.parse().ok()?;
    let boot: u64 = fs::read_to_string(proc_root.join("stat")).ok()?.lines().find_map(|line| line.strip_prefix("btime ")?.trim().parse().ok())?;
    Some((boot + ticks / USER_HZ) * 1000)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
}
