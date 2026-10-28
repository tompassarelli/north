use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

pub(super) enum Stop {
    Usage(String),
    Fail(String),
    Refused,
}

impl From<io::Error> for Stop {
    fn from(error: io::Error) -> Self {
        Self::Fail(format!("mutant: {error}"))
    }
}

pub(super) enum Lease {
    Inherited,
    Helper(PathBuf),
    Missing(Vec<PathBuf>),
}

pub(super) struct Config {
    pub(super) run: String,
    pub(super) class: String,
    pub(super) links: Vec<String>,
}

pub(super) struct Env {
    pub(super) state_home: PathBuf,
    pub(super) cache_home: PathBuf,
    pub(super) lease: Lease,
}

impl Env {
    pub(super) fn mutants(&self, repo: &str) -> PathBuf {
        self.state_home.join("agents/mutants").join(repo)
    }

    pub(super) fn patch(&self, repo: &str, id: &str) -> PathBuf {
        self.mutants(repo).join(format!("{id}.patch"))
    }
}

pub(super) fn valid_id(id: &str) -> bool {
    id.chars().next().is_some_and(|first| first.is_ascii_alphanumeric())
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn bare(dir: &Path) -> Option<bool> {
    let output = git(dir, None, &["rev-parse", "--is-bare-repository"]).ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).trim() == "true")
}

pub(super) fn repo_name(cwd: &Path) -> Result<String, Stop> {
    let mut origin = git_text(cwd, None, &["remote", "get-url", "origin"])?;
    for _ in 0..5 {
        let dir = PathBuf::from(origin.strip_prefix("file://").unwrap_or(&origin));
        if !(dir.is_dir() && bare(&dir) == Some(false)) {
            break;
        }
        origin = git_text(&dir, None, &["remote", "get-url", "origin"])
            .map_err(|_| Stop::Usage(format!("mutant: origin {} is a clone with no origin of its own", dir.display())))?;
    }
    let dir = Path::new(origin.strip_prefix("file://").unwrap_or(&origin));
    if dir.is_dir() && bare(dir) != Some(true) {
        return Err(Stop::Usage(format!("mutant: cannot resolve the repository behind origin {origin}")));
    }
    let tail = origin.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
    let name = tail.strip_suffix(".git").unwrap_or(tail);
    let valid = name.chars().next().is_some_and(|first| first.is_ascii_alphanumeric())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if !valid {
        return Err(Stop::Usage("mutant: invalid repository name".to_owned()));
    }
    Ok(name.to_owned())
}

pub(super) fn git(cwd: &Path, index: Option<&Path>, args: &[&str]) -> io::Result<Output> {
    let mut command = Command::new("git");
    command.current_dir(cwd).args(args).stdin(Stdio::null());
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    command.output()
}

pub(super) fn git_piped(cwd: &Path, args: &[&str], input: &[u8]) -> io::Result<Output> {
    let mut child = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or_else(|| io::Error::other("git stdin unavailable"))?;
    stdin.write_all(input)?;
    drop(stdin);
    child.wait_with_output()
}

pub(super) fn git_bytes(cwd: &Path, index: Option<&Path>, args: &[&str]) -> Result<Vec<u8>, Stop> {
    let output = git(cwd, index, args)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Stop::Fail(format!("mutant: git {} failed: {}", args.join(" "), stderr.trim())));
    }
    Ok(output.stdout)
}

pub(super) fn git_text(cwd: &Path, index: Option<&Path>, args: &[&str]) -> Result<String, Stop> {
    Ok(String::from_utf8_lossy(&git_bytes(cwd, index, args)?).trim().to_owned())
}

