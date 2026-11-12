use std::fmt;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use crate::platform::git as platform_git;
use crate::platform::process;

const SOUTH_REF: &str = "refs/south/main";
const SOUTH_DENYLIST_PATH: &str = "privacy/public-denylist.toml";
const FETCH_LIMIT: Duration = Duration::from_secs(90);
const STALE_LOCK: Duration = Duration::from_secs(180);
const LOCK: &str = "north-fetch.lock";
const STAMP: &str = "north-last-fetch";

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
    let main = south.join("main");
    if let Err(reason) = origin_url(&main).and_then(|url| fetch(mirror, &url)) {
        if !holds_copy(mirror) {
            return Err(Fault::Unavailable(format!("cannot fetch the deny-list into {} and it holds no earlier copy: {reason}", mirror.display())));
        }
        eprintln!("denylist: ⚠ cannot refresh the deny-list ({reason}); scanning with the copy fetched {} ago", age(mirror));
    }
    let spec = format!("{SOUTH_REF}:{SOUTH_DENYLIST_PATH}");
    let rules = platform_git::stdout(mirror, &["--no-replace-objects", "--git-dir", ".", "show", &spec], |output| String::from_utf8_lossy(&output.stderr).trim().to_owned())
        .map_err(|e| Fault::Failed(format!("the deny-list is missing from {} {spec}: {e}", mirror.display())))?;
    let mut file = tempfile::NamedTempFile::new().map_err(|e| Fault::Failed(format!("cannot create the deny-list copy: {e}")))?;
    file.write_all(&rules).map_err(|e| Fault::Failed(format!("cannot write the deny-list copy: {e}")))?;
    Ok(file)
}

fn origin_url(main: &Path) -> Result<String, String> {
    let url = platform_git::stdout(main, &["remote", "get-url", "origin"], |output| String::from_utf8_lossy(&output.stderr).trim().to_owned())
        .map_err(|e| format!("cannot read south's origin URL in {}: {e}", main.display()))?;
    Ok(String::from_utf8_lossy(&url).trim().to_owned())
}

fn holds_copy(mirror: &Path) -> bool {
    platform_git::run(mirror, &["--git-dir", ".", "rev-parse", "--verify", "-q", SOUTH_REF]).is_ok_and(|output| output.status.success())
}

fn age(mirror: &Path) -> String {
    let seconds = std::fs::metadata(mirror.join(STAMP)).and_then(|stamp| stamp.modified()).ok().and_then(|at| SystemTime::now().duration_since(at).ok()).map(|elapsed| elapsed.as_secs());
    match seconds {
        None => "an unknown time".to_owned(),
        Some(seconds) if seconds < 120 => format!("{seconds} s"),
        Some(seconds) if seconds < 7200 => format!("{} min", seconds / 60),
        Some(seconds) if seconds < 172_800 => format!("{} h", seconds / 3600),
        Some(seconds) => format!("{} days", seconds / 86_400),
    }
}

struct Lock(PathBuf);

impl Lock {
    fn take(path: &Path) -> Result<Self, String> {
        for _ in 0..2 {
            match OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(_) => return Ok(Self(path.to_owned())),
                Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                    let held = std::fs::metadata(path).and_then(|lock| lock.modified()).ok().and_then(|at| SystemTime::now().duration_since(at).ok());
                    if held.is_some_and(|held| held < STALE_LOCK) {
                        return Err(format!("another scan holds {}", path.display()));
                    }
                    let _ = std::fs::remove_file(path);
                }
                Err(e) => return Err(format!("cannot take {}: {e}", path.display())),
            }
        }
        Err(format!("cannot take {}", path.display()))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn fetch(mirror: &Path, url: &str) -> Result<(), String> {
    let init = platform_git::run(Path::new("."), &["init", "--bare", "-q", &mirror.to_string_lossy()]).map_err(|e| e.to_string())?;
    if !init.status.success() {
        return Err(format!("git init failed in {}: {}", mirror.display(), String::from_utf8_lossy(&init.stderr).trim()));
    }
    let _lock = Lock::take(&mirror.join(LOCK))?;
    for left_by_a_killed_fetch in [format!("{SOUTH_REF}.lock"), "packed-refs.lock".to_owned()] {
        let _ = std::fs::remove_file(mirror.join(left_by_a_killed_fetch));
    }
    let mut command = Command::new("git");
    command.arg("--git-dir").arg(mirror).args(["fetch", "-q", "--no-tags", "--", url, &format!("+refs/heads/main:{SOUTH_REF}")]).env("GIT_TERMINAL_PROMPT", "0");
    let output = match process::try_run_group_traced_within(command, FETCH_LIMIT).map_err(|e| e.to_string())? {
        Ok(output) => output,
        Err(_) => return Err(format!("git fetch did not finish within {} s", FETCH_LIMIT.as_secs())),
    };
    if !output.status.success() {
        return Err(format!("git fetch failed (exit {}): {}", output.status.code().unwrap_or(-1), String::from_utf8_lossy(&output.stderr).trim()));
    }
    std::fs::write(mirror.join(STAMP), b"").map_err(|e| format!("cannot record the fetch time: {e}"))
}
