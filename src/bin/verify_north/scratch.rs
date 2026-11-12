use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::exec;

pub const PREFIX: &str = "verify-north-";
pub const BRANCH: &str = "worker";
const QUERY_WAIT: Duration = Duration::from_secs(30);
const USAGE_GATE: &str = "agents/usage-gate.json";
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
    let state = directory.keep().canonicalize().map_err(|error| format!("state directory: {error}"))?;
    seed_usage_gate(&live_state()?, &state)?;
    Ok(state)
}

/// Copies the host's live usage gate into the walk's state, so its worker is placed on the plan account the host would pick: with no gate there every plan account read as held for want of a usage sample, and the walk sat in placement past the canary's limit (north#1273). A copy, so nothing the walk writes reaches the host's state.
fn seed_usage_gate(live: &Path, state: &Path) -> Result<(), String> {
    let from = live.join(USAGE_GATE);
    if !from.is_file() {
        return Ok(());
    }
    let agents = state.join("agents");
    fs::create_dir_all(&agents).map_err(|error| format!("{}: {error}", agents.display()))?;
    fs::copy(&from, state.join(USAGE_GATE)).map(drop).map_err(|error| format!("{}: {error}", from.display()))
}

fn live_state() -> Result<PathBuf, String> {
    std::env::var_os("XDG_STATE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .ok_or_else(|| "no state directory".to_owned())
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

    pub fn origin_main(&self, limit: Duration) -> Result<String, String> {
        git_within(&self.origin, &["rev-parse", "refs/heads/main"], limit)
    }

    pub fn lane_head(&self, limit: Duration) -> Result<String, String> {
        git_within(&self.lane, &["rev-parse", "HEAD"], limit)
    }

    pub fn origin(&self, arguments: &[&str]) -> Result<String, String> {
        git(&self.origin, arguments)
    }
}

pub fn keep_ledger(root: &Path, stamp: u64) -> Result<PathBuf, String> {
    let state = live_state()?;
    let kept = state.join("north/verify-north/failed").join(stamp.to_string());
    fs::create_dir_all(&kept).map_err(|error| format!("{}: {error}", kept.display()))?;
    for name in ["threads.db", "threads.db-wal", "threads.db-shm", "pushes.tsv", "lander.log", "broker.log"] {
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
    git_within(directory, arguments, QUERY_WAIT)
}

fn git_within(directory: &Path, arguments: &[&str], limit: Duration) -> Result<String, String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(directory).args(arguments);
    Ok(exec::stdout(command, limit, None)?.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_walk_state_carries_the_live_usage_gate_where_placement_reads_it() {
        let (live, state) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        fs::create_dir_all(live.path().join("agents")).unwrap();
        fs::write(live.path().join(USAGE_GATE), "{\"claude\":{}}\n").unwrap();
        seed_usage_gate(live.path(), state.path()).unwrap();
        assert_eq!(fs::read_to_string(state.path().join("agents/usage-gate.json")).unwrap(), "{\"claude\":{}}\n", "placement reads XDG_STATE_HOME/agents/usage-gate.json; without it every plan account reads as held");
    }
}
