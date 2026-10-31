//! The commands and files the flow readings come from: every external effect goes through `Host`
//! and `Places`, so the tests substitute both.
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use crate::platform::process;

const LIMIT: Duration = Duration::from_secs(20);

pub trait Host {
    fn output(&self, argv: &[&str], cwd: Option<&Path>, input: Option<&str>, limit: Duration) -> Option<Output>;

    /// The command's stdout when it exits 0 within twenty seconds.
    fn run(&self, argv: &[&str], cwd: Option<&Path>, input: Option<&str>) -> Option<String> {
        let output = self.output(argv, cwd, input, LIMIT)?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn json(&self, argv: &[&str]) -> Option<Value> {
        serde_json::from_str(&self.run(argv, None, None)?).ok()
    }

    fn graphql(&self, query: &str) -> Option<Value> {
        let input = json!({ "query": query }).to_string();
        serde_json::from_str(&self.run(&["gh", "api", "graphql", "--input", "-"], None, Some(&input))?).ok()
    }
}

#[derive(Default)]
pub struct Machine {
    github_calls: AtomicUsize,
}

impl Machine {
    pub fn github_calls(&self) -> usize {
        self.github_calls.load(Ordering::Relaxed)
    }
}

impl Host for Machine {
    fn output(&self, argv: &[&str], cwd: Option<&Path>, input: Option<&str>, limit: Duration) -> Option<Output> {
        if argv.first() == Some(&"gh") {
            self.github_calls.fetch_add(1, Ordering::Relaxed);
        }
        let mut command = Command::new(argv.first()?);
        command.args(&argv[1..]);
        command.env("PATH", with_user_bin(std::env::var_os("PATH"), &std::env::var_os("HOME").unwrap_or_default()));
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        process::run_command_within(command, input.map(str::as_bytes), limit)
    }
}

/// PATH with the user's `~/.local/bin` last, where `claude` lives and a systemd unit's PATH does not reach.
pub(super) fn with_user_bin(path: Option<std::ffi::OsString>, home: &std::ffi::OsStr) -> std::ffi::OsString {
    let mut paths: Vec<PathBuf> = path.iter().flat_map(std::env::split_paths).collect();
    let user_bin = PathBuf::from(home).join(".local/bin");
    if !paths.contains(&user_bin) {
        paths.push(user_bin);
    }
    std::env::join_paths(paths).unwrap_or_default()
}

#[derive(Clone, Debug)]
pub struct Places {
    pub home: PathBuf,
    pub state: PathBuf,
    pub ledger: PathBuf,
}

impl Places {
    pub fn from_env() -> Self {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| home.join(".local/state"), PathBuf::from);
        let ledger = std::env::var_os("THREADS_DB").map_or_else(
            || state.join("threads").join(if home.join(".config/threads/remote").exists() { "snapshot.db" } else { "threads.db" }),
            PathBuf::from,
        );
        Self { home, state, ledger }
    }

    pub fn flow(&self) -> PathBuf {
        self.state.join("agents/flow")
    }

    pub fn checkout(&self, repo: &str) -> PathBuf {
        self.home.join("code").join(repo).join("main")
    }
}

pub fn valid_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters.next().is_some_and(|first| first.is_ascii_alphanumeric())
        && characters.all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | '-'))
}

pub fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&temporary, text)?;
    fs::rename(temporary, path)
}

pub fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

pub fn seconds(text: &str) -> Option<f64> {
    crate::foundation::clock::Timestamp::parse_python_datetime(text).map(|at| at.unix_seconds_f64())
}

pub fn iso(at: f64) -> String {
    crate::foundation::clock::Timestamp::from_unix_seconds_f64(at).iso()
}

pub fn median(samples: &[f64]) -> Option<f64> {
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    match sorted.len() {
        0 => None,
        length if length % 2 == 1 => Some(sorted[middle]),
        _ => Some((sorted[middle - 1] + sorted[middle]) / 2.0),
    }
}
