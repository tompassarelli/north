//! Local agent starts: the one door from the operator's fleet to a session as the agent user on this host. A worker writes a start request into its own spool location; the starter, a system unit running as the operator outside every session scope, reads each request as fixed fields, never a command, and hands the validated request on standard input to a fixed entry it opens as the agent through machined's host shell, which polkit admits for that unit alone; the entry reads the same fixed fields and records the request. It depends on no other north crate, so the host configuration builds it alone into the store, and the unit runs nothing the operator can write.
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// The extension of a start request in a session's spool location.
pub const REQUEST: &str = "start";
/// The line the entry prints once it reads its terminal, before which the starter writes nothing.
pub const READY: &str = "north-agent-starts: ready";
const SESSIONS: &str = "sessions";
const REJECTED: &str = "rejected";
const RECORDS: &str = ".local/state/north/agent-starts";
const LIMIT: u64 = 4096;
const API: &str = "api";
const NAME_LIMIT: usize = 64;
const PATH_LIMIT: usize = 1024;

/// A request to start one agent session: its name, the account it runs on, its working directory and its brief file, both beneath the agent's home, and nothing else.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartRequest {
    pub name: String,
    pub account: String,
    pub cwd: PathBuf,
    pub brief: PathBuf,
}

impl StartRequest {
    /// The request `bytes` hold, refused unless they are exactly these four fields, the name and account plain names, the account no API account, and both paths absolute and beneath `home` with no `..`.
    pub fn parse(bytes: &[u8], home: &Path) -> Result<Self, String> {
        let request: Self = serde_json::from_slice(bytes).map_err(|error| format!("not a start request: {error}"))?;
        plain_name("name", &request.name)?;
        plain_name("account", &request.account)?;
        subscription(&request.account)?;
        beneath("cwd", &request.cwd, home)?;
        beneath("brief", &request.brief, home)?;
        Ok(request)
    }

    /// The request as the one line the entry reads, written from the parsed fields rather than the spooled bytes.
    pub fn line(&self) -> String {
        let mut line = serde_json::to_string(self).unwrap_or_default();
        line.push('\n');
        line
    }
}

fn plain_name(field: &str, value: &str) -> Result<(), String> {
    let plain = !value.is_empty() && value.len() <= NAME_LIMIT && !value.starts_with(['-', '.']) && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if plain { Ok(()) } else { Err(format!("{field} {value:?} is not a plain name")) }
}

fn subscription(account: &str) -> Result<(), String> {
    if account.get(..API.len()).is_some_and(|prefix| prefix.eq_ignore_ascii_case(API)) { Err(format!("account {account:?} is an API account, and agent sessions start on subscription accounts only")) } else { Ok(()) }
}

fn beneath(field: &str, path: &Path, home: &Path) -> Result<(), String> {
    let text = path.to_str().unwrap_or("\0");
    let plain = path.is_absolute() && text.len() <= PATH_LIMIT && !text.chars().any(char::is_control) && path.components().skip(1).all(|part| matches!(part, Component::Normal(_)));
    if plain && path.starts_with(home) && path != home { Ok(()) } else { Err(format!("{field} {text:?} is not a plain path beneath {}", home.display())) }
}

/// The fleet spool the starter reads: each session's own location beneath `sessions`, which only that session's worker writes.
pub struct Spool {
    dir: PathBuf,
}

impl Spool {
    pub fn at(dir: &Path) -> Self {
        Self { dir: dir.to_path_buf() }
    }

    /// Every start request waiting in a session's location, oldest name first.
    pub fn waiting(&self) -> io::Result<Vec<PathBuf>> {
        let mut found = Vec::new();
        for session in entries(&self.dir.join(SESSIONS))? {
            if fs::symlink_metadata(&session).is_ok_and(|meta| meta.is_dir()) {
                found.extend(entries(&session)?.into_iter().filter(|path| path.extension().is_some_and(|extension| extension == REQUEST)));
            }
        }
        found.sort();
        Ok(found)
    }

    /// The bytes of the request at `path` when it is a regular file, else none, the entry then set aside under `rejected`.
    pub fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        if !fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file()) {
            self.set_aside(path)?;
            return Ok(None);
        }
        let file = fs::OpenOptions::new().read(true).custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32).open(path)?;
        let mut bytes = Vec::new();
        file.take(LIMIT).read_to_end(&mut bytes)?;
        Ok(Some(bytes))
    }

    pub fn set_aside(&self, path: &Path) -> io::Result<()> {
        let (Some(name), Some(session)) = (path.file_name(), path.parent().and_then(Path::file_name)) else { return Ok(()) };
        let aside = self.dir.join(REJECTED);
        fs::create_dir_all(&aside)?;
        let mut target = session.to_os_string();
        target.push("-");
        target.push(name);
        match fs::rename(path, aside.join(target)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}

fn entries(dir: &Path) -> io::Result<Vec<PathBuf>> {
    match fs::read_dir(dir) {
        Ok(entries) => Ok(entries.flatten().map(|entry| entry.path()).collect()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

/// How the starter opens the entry as the agent: `machinectl` and `entry` are fixed store paths and `agent` and `home` the agent user's, all from the unit, none from the spool.
pub struct Starter {
    pub machinectl: PathBuf,
    pub agent: String,
    pub home: PathBuf,
    pub entry: PathBuf,
    pub wait: Duration,
}

impl Starter {
    /// `machinectl shell AGENT@.host ENTRY` with no arguments after the entry, so polkit's command_line detail equals its program, and an empty environment.
    pub fn command(&self) -> Command {
        let mut command = Command::new(&self.machinectl);
        command.args(["--quiet", "--no-ask-password", "shell"]).arg(format!("{}@.host", self.agent)).arg(&self.entry).env_clear().stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        command
    }

    /// Starts every waiting request once, each line it reports written to `log`.
    pub fn serve_once(&self, spool: &Spool, log: &mut dyn Write) -> io::Result<()> {
        for path in spool.waiting()? {
            let Some(bytes) = spool.read(&path)? else {
                writeln!(log, "set aside {}: not a regular file", path.display())?;
                continue;
            };
            match StartRequest::parse(&bytes, &self.home) {
                Ok(request) => {
                    fs::remove_file(&path)?;
                    writeln!(log, "{} {}: {}", request.name, path.display(), self.start(&request))?;
                }
                Err(why) => {
                    spool.set_aside(&path)?;
                    writeln!(log, "refused {}: {why}", path.display())?;
                }
            }
        }
        Ok(())
    }

    fn start(&self, request: &StartRequest) -> String {
        let mut child = match self.command().spawn() {
            Ok(child) => child,
            Err(error) => return format!("cannot run machinectl: {error}"),
        };
        let (ready, readied) = std::sync::mpsc::channel();
        let said = child.stdout.take().map(|out| {
            std::thread::spawn(move || {
                let mut said = Vec::new();
                let mut lines = BufReader::new(out);
                let mut line = Vec::new();
                while (&mut lines).take(LIMIT).read_until(b'\n', &mut line).is_ok_and(|read| read > 0) {
                    if line.trim_ascii() == READY.as_bytes() {
                        let _ = ready.send(());
                    }
                    said = std::mem::take(&mut line);
                }
                String::from_utf8_lossy(&said).chars().filter(|character| !character.is_control()).collect::<String>()
            })
        });
        let deadline = Instant::now() + self.wait;
        let mut input = child.stdin.take();
        if readied.recv_timeout(self.wait).is_ok()
            && let Some(input) = input.as_mut()
        {
            let _ = input.write_all(request.line().as_bytes());
        }
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    drop(input);
                    let last = said.and_then(|reader| reader.join().ok()).unwrap_or_default();
                    return format!("the entry exited {status}: {last}");
                }
                Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(50)),
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return format!("the entry did not finish within {} s", self.wait.as_secs());
                }
            }
        }
    }
}

/// The entry's side, run as the agent: says [`READY`] on `output` once its terminal is its own, so no line arrives before machined's terminal reset flushes it, then reads one line of `input` as data, parses the fixed fields against the agent's `home` and records the request under it, starting nothing else.
pub fn record(input: &mut dyn Read, output: &mut dyn Write, home: &Path) -> Result<StartRequest, String> {
    writeln!(output, "{READY}").and_then(|()| output.flush()).map_err(|error| format!("cannot say ready: {error}"))?;
    let mut line = Vec::new();
    BufReader::new(input.take(LIMIT)).read_until(b'\n', &mut line).map_err(|error| format!("cannot read the request: {error}"))?;
    let request = StartRequest::parse(line.trim_ascii_end(), home)?;
    let records = home.join(RECORDS);
    fs::create_dir_all(&records).map_err(|error| format!("cannot make {}: {error}", records.display()))?;
    let (staged, file) = (records.join(format!(".{}.json", request.name)), records.join(format!("{}.json", request.name)));
    fs::write(&staged, request.line()).and_then(|()| fs::rename(&staged, &file)).map_err(|error| format!("cannot record {}: {error}", file.display()))?;
    Ok(request)
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
