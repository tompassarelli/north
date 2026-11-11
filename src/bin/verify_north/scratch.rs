use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::exec;

pub const PREFIX: &str = "verify-north-";
pub const BRANCH: &str = "worker";
const QUERY_WAIT: Duration = Duration::from_secs(30);
const IDENTITY: [(&str, &str); 3] = [("user.name", "verify-north"), ("user.email", "verify-north@example.invalid"), ("commit.gpgsign", "false")];

pub struct Scratch {
    pub root: PathBuf,
    pub origin: PathBuf,
    pub main: PathBuf,
    pub lane: PathBuf,
    pub broker: PathBuf,
    pub base: String,
}

pub fn state_root() -> Result<PathBuf, String> {
    let directory = tempfile::Builder::new().prefix(PREFIX).tempdir().map_err(|error| format!("state directory: {error}"))?;
    directory.keep().canonicalize().map_err(|error| format!("state directory: {error}"))
}

impl Scratch {
    pub fn create() -> Result<Self, String> {
        let directory = tempfile::Builder::new().prefix(PREFIX).tempdir().map_err(|error| format!("scratch directory: {error}"))?;
        let root = directory.keep().canonicalize().map_err(|error| format!("scratch directory: {error}"))?;
        let (origin, main, lane) = (root.join("origin.git"), root.join("project/main"), root.join("project/worktrees").join(BRANCH));
        fs::create_dir_all(&main).map_err(|error| format!("{}: {error}", main.display()))?;
        git(&main, &["init", "-q", "-b", "main"])?;
        for (key, value) in IDENTITY {
            git(&main, &["config", key, value])?;
        }
        write(&main.join("README.md"), "The scratch repository verify-north lands one worker's change into.\n")?;
        write(&main.join(".safe-push"), &format!("[landing]\n\tqueue = lander\n\tlander-host = {}\n\tremote-hosts =\n[land]\n\tcheck = ** :: true\n", host()?))?;
        git(&main, &["add", "."])?;
        git(&main, &["commit", "-q", "-m", "Start the scratch repository"])?;
        git(&root, &["clone", "-q", "--bare", &path(&main), &path(&origin)])?;
        git(&main, &["remote", "add", "origin", &path(&origin)])?;
        git(&main, &["fetch", "-q", "origin"])?;
        git(&main, &["worktree", "add", "-q", "-b", BRANCH, &path(&lane), "refs/remotes/origin/main"])?;
        let base = git(&origin, &["rev-parse", "refs/heads/main"])?;
        let broker = root.join("broker.sock");
        Ok(Self { root, origin, main, lane, broker, base })
    }

    pub fn origin_main(&self) -> Result<String, String> {
        git(&self.origin, &["rev-parse", "refs/heads/main"])
    }

    pub fn lane_head(&self) -> Result<String, String> {
        git(&self.lane, &["rev-parse", "HEAD"])
    }

    pub fn origin(&self, arguments: &[&str]) -> Result<String, String> {
        git(&self.origin, arguments)
    }
}

pub fn keep_ledger(root: &Path, stamp: u64) -> Result<PathBuf, String> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .ok_or("no state directory")?;
    let kept = state.join("north/verify-north/failed").join(stamp.to_string());
    fs::create_dir_all(&kept).map_err(|error| format!("{}: {error}", kept.display()))?;
    for name in ["threads.db", "threads.db-wal", "threads.db-shm", "pushes.tsv"] {
        let from = root.join(name);
        if from.exists() {
            fs::copy(&from, kept.join(name)).map_err(|error| format!("{}: {error}", from.display()))?;
        }
    }
    Ok(kept)
}

pub fn removable(path: &Path) -> bool {
    path.parent() == Some(std::env::temp_dir().canonicalize().unwrap_or_else(|_| std::env::temp_dir()).as_path())
        && path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.len() > PREFIX.len() && name.starts_with(PREFIX))
}

fn host() -> Result<String, String> {
    let name = fs::read_to_string("/proc/sys/kernel/hostname").map_err(|error| format!("host name: {error}"))?;
    Ok(name.trim().split('.').next().unwrap_or_default().to_owned())
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}

fn path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn git(directory: &Path, arguments: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(directory).args(arguments);
    Ok(exec::stdout(command, QUERY_WAIT, None)?.trim().to_owned())
}
