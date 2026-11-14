use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use rustix::fs::{FlockOperation, flock};

use super::south_policy;
use crate::platform::betterleaks;
use crate::platform::south_land_limits::{MAX_COMMITS, MAX_PATCH_BYTES};
use crate::fleet::attachments::base64;

pub const TOKEN_FILE: &str = "github-token";
const BRANCH: &str = "main";
const REMOTE_REF: &str = "refs/remotes/origin/main";

#[derive(Clone, Debug)]
pub struct Config {
    pub state_dir: PathBuf,
    pub remote: String,
    pub betterleaks: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Landed {
    pub sha: String,
}

#[derive(Debug, Eq, PartialEq)]
pub enum Refusal {
    TooLarge(usize),
    TooManyCommits(usize),
    Busy,
    Credentials(String),
    Apply(String),
    Empty,
    Policy(south_policy::Refusal),
    Secrets(String),
    Git { step: &'static str, detail: String },
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge(bytes) => write!(formatter, "patch of {bytes} bytes is over the {MAX_PATCH_BYTES} byte limit"),
            Self::TooManyCommits(count) => write!(formatter, "{count} commits is over the {MAX_COMMITS} commit limit"),
            Self::Busy => write!(formatter, "another south land is running"),
            Self::Credentials(detail) => write!(formatter, "no usable token: {detail}"),
            Self::Apply(detail) => write!(formatter, "patch does not apply to origin/{BRANCH}: {detail}"),
            Self::Empty => write!(formatter, "patch carries no commits"),
            Self::Policy(refusal) => write!(formatter, "{refusal}"),
            Self::Secrets(detail) => write!(formatter, "secret scan failed: {detail}"),
            Self::Git { step, detail } => write!(formatter, "git {step} failed: {detail}"),
        }
    }
}

pub fn land(config: &Config, environment: &BTreeMap<String, String>, mbox: &[u8]) -> Result<Landed, Refusal> {
    if mbox.len() > MAX_PATCH_BYTES {
        return Err(Refusal::TooLarge(mbox.len()));
    }
    let announced = announced_commits(mbox);
    if announced > MAX_COMMITS {
        return Err(Refusal::TooManyCommits(announced));
    }
    let token = read_token(environment)?;
    fs::create_dir_all(&config.state_dir).map_err(|error| git_error("state", &error.to_string()))?;
    let _lock = take_lock(&config.state_dir)?;
    let session = Session::new(config, environment, &token)?;
    session.sync()?;
    session.apply(mbox)?;
    let commits = session.commit_count()?;
    if commits == 0 {
        return Err(Refusal::Empty);
    }
    if commits > MAX_COMMITS {
        return Err(Refusal::TooManyCommits(commits));
    }
    south_policy::check_raw_diff(&session.raw_diff()?).map_err(Refusal::Policy)?;
    session.scan_secrets(&config.betterleaks)?;
    session.push()?;
    Ok(Landed { sha: session.head()? })
}

pub fn process_environment() -> BTreeMap<String, String> {
    std::env::vars().collect()
}

fn announced_commits(mbox: &[u8]) -> usize {
    mbox.split(|byte| *byte == b'\n')
        .filter(|line| line.len() > 45 && line.starts_with(b"From ") && line[5..45].iter().all(u8::is_ascii_hexdigit) && line[45] == b' ')
        .count()
}

fn read_token(environment: &BTreeMap<String, String>) -> Result<String, Refusal> {
    let directory = environment.get("CREDENTIALS_DIRECTORY").ok_or_else(|| Refusal::Credentials("CREDENTIALS_DIRECTORY is not set".to_owned()))?;
    let path = Path::new(directory).join(TOKEN_FILE);
    let token = fs::read_to_string(&path).map_err(|error| Refusal::Credentials(format!("{}: {error}", path.display())))?;
    let token = token.trim().to_owned();
    if token.is_empty() { Err(Refusal::Credentials(format!("{} is empty", path.display()))) } else { Ok(token) }
}

fn take_lock(state_dir: &Path) -> Result<File, Refusal> {
    let lock = OpenOptions::new().create(true).truncate(false).write(true).open(state_dir.join("lock")).map_err(|error| git_error("lock", &error.to_string()))?;
    flock(&lock, FlockOperation::NonBlockingLockExclusive).map_err(|_| Refusal::Busy)?;
    Ok(lock)
}

fn git_error(step: &'static str, detail: &str) -> Refusal {
    Refusal::Git { step, detail: detail.trim().to_owned() }
}

struct Session<'a> {
    clone: PathBuf,
    remote: &'a str,
    path: String,
    home: &'a Path,
    token: &'a str,
}

impl<'a> Session<'a> {
    fn new(config: &'a Config, environment: &BTreeMap<String, String>, token: &'a str) -> Result<Self, Refusal> {
        let path = environment.get("PATH").cloned().ok_or_else(|| git_error("environment", "PATH is not set"))?;
        Ok(Self { clone: config.state_dir.join("clone"), remote: &config.remote, path, home: &config.state_dir, token })
    }

    fn command(&self, program: &Path, authenticated: bool) -> Command {
        let mut command = Command::new(program);
        command
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", self.home)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if authenticated {
            let header = format!("Authorization: Basic {}", base64(format!("x-access-token:{}", self.token).as_bytes()));
            command
                .env("GIT_CONFIG_COUNT", "1")
                .env("GIT_CONFIG_KEY_0", format!("http.{}.extraHeader", self.remote))
                .env("GIT_CONFIG_VALUE_0", header);
        }
        command
    }

    fn git(&self, step: &'static str, arguments: &[&str], authenticated: bool, stdin: Option<&[u8]>) -> Result<Output, Refusal> {
        let mut command = self.command(Path::new("git"), authenticated);
        command
            .current_dir(&self.clone)
            .args(["-c", "core.hooksPath=/dev/null", "-c", "user.name=south-land", "-c", "user.email=south-land@localhost"])
            .args(["-c", "commit.gpgsign=false", "-c", "core.fsmonitor=false"])
            .args(arguments);
        if stdin.is_some() {
            command.stdin(Stdio::piped());
        }
        let mut child = command.spawn().map_err(|error| git_error(step, &error.to_string()))?;
        if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(bytes).map_err(|error| git_error(step, &error.to_string()))?;
        }
        child.wait_with_output().map_err(|error| git_error(step, &error.to_string()))
    }

    fn git_ok(&self, step: &'static str, arguments: &[&str], authenticated: bool) -> Result<Output, Refusal> {
        let output = self.git(step, arguments, authenticated, None)?;
        if output.status.success() { Ok(output) } else { Err(self.failure(step, &output)) }
    }

    fn failure(&self, step: &'static str, output: &Output) -> Refusal {
        let detail = String::from_utf8_lossy(&output.stderr).replace(self.token, "***");
        git_error(step, &detail)
    }

    fn sync(&self) -> Result<(), Refusal> {
        fs::create_dir_all(&self.clone).map_err(|error| git_error("init", &error.to_string()))?;
        self.git_ok("init", &["init", "-q"], false)?;
        if !self.git("remote", &["remote", "set-url", "origin", self.remote], false, None)?.status.success() {
            self.git_ok("remote", &["remote", "add", "origin", self.remote], false)?;
        }
        let _ = self.git("am", &["am", "--abort"], false, None);
        let refspec = format!("+refs/heads/{BRANCH}:{REMOTE_REF}");
        self.git_ok("fetch", &["fetch", "-q", "--no-tags", "origin", &refspec], true)?;
        self.git_ok("checkout", &["checkout", "-q", "-f", "--detach", REMOTE_REF], false)?;
        self.git_ok("clean", &["clean", "-ffdxq"], false)?;
        Ok(())
    }

    fn apply(&self, mbox: &[u8]) -> Result<(), Refusal> {
        let output = self.git("am", &["am", "--3way", "--quiet"], false, Some(mbox))?;
        if output.status.success() {
            return Ok(());
        }
        let detail = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        let _ = self.git("am", &["am", "--abort"], false, None);
        Err(Refusal::Apply(detail.trim().to_owned()))
    }

    fn commit_count(&self) -> Result<usize, Refusal> {
        let range = format!("{REMOTE_REF}..HEAD");
        let output = self.git_ok("rev-list", &["rev-list", "--count", &range], false)?;
        String::from_utf8_lossy(&output.stdout).trim().parse().map_err(|_| git_error("rev-list", "count is not a number"))
    }

    fn raw_diff(&self) -> Result<Vec<u8>, Refusal> {
        let range = format!("{REMOTE_REF}..HEAD");
        Ok(self.git_ok("diff", &["diff", "--raw", "--no-renames", "-z", &range], false)?.stdout)
    }

    fn scan_secrets(&self, betterleaks: &Path) -> Result<(), Refusal> {
        let range = format!("{REMOTE_REF}..HEAD");
        let output = self
            .command(betterleaks, false)
            .current_dir(&self.clone)
            .env(betterleaks::CONFIG_ENV, betterleaks::SECRET_CONFIG)
            .args(betterleaks::history_args(&range))
            .output()
            .map_err(|error| Refusal::Secrets(error.to_string()))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(Refusal::Secrets(format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)).trim().to_owned()))
        }
    }

    fn push(&self) -> Result<(), Refusal> {
        let destination = format!("HEAD:refs/heads/{BRANCH}");
        self.git_ok("push", &["push", "-q", "origin", &destination], true).map(drop)
    }

    fn head(&self) -> Result<String, Refusal> {
        let output = self.git_ok("rev-parse", &["rev-parse", "HEAD"], false)?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }
}
