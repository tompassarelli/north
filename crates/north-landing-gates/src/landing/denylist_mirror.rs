use std::fmt;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::platform::git as platform_git;
use crate::platform::process;

const SOUTH_REF: &str = "refs/south/main";
const SOUTH_DENYLIST_PATH: &str = "privacy/public-denylist.toml";
const FETCH_LIMIT: Duration = Duration::from_secs(90);
const STALE_LOCK: Duration = Duration::from_secs(180);
const WAIT_STEP: Duration = Duration::from_millis(20);
const LOCK: &str = "north-fetch.lock";
const FETCHED: &str = "north-last-fetch";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Fault {
    Unavailable(String),
    Failed(String),
}

impl fmt::Display for Fault {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (Self::Unavailable(message) | Self::Failed(message)) = self;
        formatter.write_str(message)
    }
}

impl From<String> for Fault {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

pub fn mirror_path() -> Result<PathBuf, Fault> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .ok_or_else(|| Fault::Unavailable("neither XDG_CACHE_HOME nor HOME is set, so there is no place for the deny-list mirror".to_owned()))?;
    Ok(base.join("north").join("south-denylist.git"))
}

pub fn south_denylist_in(south: &Path, mirror: &Path) -> Result<tempfile::NamedTempFile, Fault> {
    let began = SystemTime::now();
    let main = south.join("main");
    let commit = origin_url(&main)
        .and_then(|url| fetched_after(mirror, &url, began))
        .map_err(|reason| Fault::Unavailable(format!("cannot fetch the deny-list into {} after this scan began, and no older copy is used: {reason}", mirror.display())))?;
    let spec = format!("{commit}:{SOUTH_DENYLIST_PATH}");
    let rules = platform_git::stdout(mirror, &["--no-replace-objects", "--git-dir", ".", "show", &spec], |output| String::from_utf8_lossy(&output.stderr).trim().to_owned())
        .map_err(|e| Fault::Failed(format!("the deny-list is missing from {} {SOUTH_REF}:{SOUTH_DENYLIST_PATH}: {e}", mirror.display())))?;
    let mut file = tempfile::NamedTempFile::new().map_err(|e| Fault::Failed(format!("cannot create the deny-list copy: {e}")))?;
    file.write_all(&rules).map_err(|e| Fault::Failed(format!("cannot write the deny-list copy: {e}")))?;
    Ok(file)
}

fn origin_url(main: &Path) -> Result<String, String> {
    let url = platform_git::stdout(main, &["remote", "get-url", "origin"], |output| String::from_utf8_lossy(&output.stderr).trim().to_owned())
        .map_err(|e| format!("cannot read south's origin URL in {}: {e}", main.display()))?;
    Ok(String::from_utf8_lossy(&url).trim().to_owned())
}

fn fetched_after(mirror: &Path, url: &str, began: SystemTime) -> Result<String, String> {
    std::fs::create_dir_all(mirror).map_err(|e| format!("cannot create {}: {e}", mirror.display()))?;
    let lock = mirror.join(LOCK);
    let deadline = Instant::now() + STALE_LOCK + FETCH_LIMIT;
    loop {
        if let Some(commit) = shared_since(mirror, began) {
            return Ok(commit);
        }
        if let Some(held) = Lock::take(&lock)? {
            return match shared_since(mirror, began) {
                Some(commit) => Ok(commit),
                None => init(mirror).and_then(|()| fetch(mirror, url, &held)),
            };
        }
        if Instant::now() >= deadline {
            return Err(format!("another scan held {} for over {} s", lock.display(), (STALE_LOCK + FETCH_LIMIT).as_secs()));
        }
        std::thread::sleep(WAIT_STEP);
    }
}

fn shared_since(mirror: &Path, began: SystemTime) -> Option<String> {
    let record = std::fs::read_to_string(mirror.join(FETCHED)).ok()?;
    let (started, commit) = record.trim().split_once(' ')?;
    let started = UNIX_EPOCH + Duration::from_nanos(started.parse().ok()?);
    (started >= began && !commit.is_empty()).then(|| commit.to_owned())
}

struct Lock(PathBuf);

impl Lock {
    fn take(path: &Path) -> Result<Option<Self>, String> {
        for _ in 0..2 {
            match OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(_) => return Ok(Some(Self(path.to_owned()))),
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    let held = std::fs::metadata(path).and_then(|lock| lock.modified()).ok().and_then(|at| SystemTime::now().duration_since(at).ok());
                    if held.is_some_and(|held| held < STALE_LOCK) {
                        return Ok(None);
                    }
                    let _ = std::fs::remove_file(path);
                }
                Err(e) => return Err(format!("cannot take {}: {e}", path.display())),
            }
        }
        Ok(None)
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn init(mirror: &Path) -> Result<(), String> {
    let init = platform_git::run(Path::new("."), &["init", "--bare", "-q", &mirror.to_string_lossy()]).map_err(|e| e.to_string())?;
    if !init.status.success() {
        return Err(format!("git init failed in {}: {}", mirror.display(), String::from_utf8_lossy(&init.stderr).trim()));
    }
    Ok(())
}

fn fetch(mirror: &Path, url: &str, _held: &Lock) -> Result<String, String> {
    for left_by_a_killed_fetch in [format!("{SOUTH_REF}.lock"), "packed-refs.lock".to_owned()] {
        let _ = std::fs::remove_file(mirror.join(left_by_a_killed_fetch));
    }
    let started = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
    let mut command = Command::new("git");
    command.arg("--git-dir").arg(mirror).args(["fetch", "-q", "--no-tags", "--", url, &format!("+refs/heads/main:{SOUTH_REF}")]).env("GIT_TERMINAL_PROMPT", "0");
    let output = match process::try_run_group_traced_within(command, FETCH_LIMIT).map_err(|e| e.to_string())? {
        Ok(output) => output,
        Err(_) => return Err(format!("git fetch did not finish within {} s", FETCH_LIMIT.as_secs())),
    };
    if !output.status.success() {
        return Err(format!("git fetch failed (exit {}): {}", output.status.code().unwrap_or(-1), String::from_utf8_lossy(&output.stderr).trim()));
    }
    let commit = platform_git::stdout(mirror, &["--git-dir", ".", "rev-parse", "--verify", &format!("{SOUTH_REF}^{{commit}}")], |output| String::from_utf8_lossy(&output.stderr).trim().to_owned())
        .map_err(|e| format!("cannot read the fetched {SOUTH_REF}: {e}"))?;
    let commit = String::from_utf8_lossy(&commit).trim().to_owned();
    let pending = mirror.join(format!("{FETCHED}.{}", std::process::id()));
    std::fs::write(&pending, format!("{started} {commit}\n"))
        .and_then(|()| std::fs::rename(&pending, mirror.join(FETCHED)))
        .map_err(|e| format!("cannot record the fetch: {e}"))?;
    Ok(commit)
}
