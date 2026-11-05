use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use rustix::process::{Pid, Signal, kill_process};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::sync::watch;
use tokio::signal::unix::{SignalKind, signal};
use tokio::task::JoinSet;

use super::select::Config;
use crate::land::after::follow::program;
use crate::land::checks::{BROKEN_BASE, ROUND};
use crate::landing::check_timings::{self, Base, BaseResult, Deadline, Timing, Timings};

const USAGE: &str = "usage: north land checks --base BASE --head HEAD [--dry 0|1] [--safe-push PATH] [--red-fix ISSUE] [--outcome FILE]\n       north land checks --portable --base BASE --head HEAD\n       north land checks --any --head HEAD\n       north land checks --after --base BASE --head HEAD [--outcome FILE]";
const OFFLOAD_GRACE: u64 = 900;
const REAP: Duration = Duration::from_secs(5);
const LOG_KEEP: Duration = Duration::from_secs(14 * 24 * 3600);
const INFRASTRUCTURE: i32 = 75;
const TIMED_OUT: i32 = 124;
const KILLED: i32 = 137;
const RED_MAIN: i32 = 3;
const CANCELLED: i32 = 130;
const CANCEL_GRACE: Duration = Duration::from_secs(15);

#[derive(Clone, Default)]
struct Arguments {
    base: String,
    head: String,
    dry: bool,
    safe_push: String,
    red_fix: String,
    outcome: Option<PathBuf>,
    portable: bool,
    any: bool,
    after: bool,
    leased: bool,
    published: String,
    runner_host: String,
    commands: Vec<String>,
}

fn parse(arguments: &[String]) -> Option<Arguments> {
    let mut parsed = Arguments::default();
    let mut rest = arguments.iter();
    while let Some(flag) = rest.next() {
        match flag.as_str() {
            "--portable" => parsed.portable = true,
            "--any" => parsed.any = true,
            "--after" => parsed.after = true,
            "--leased" => parsed.leased = true,
            "--base" | "--head" | "--dry" | "--safe-push" | "--red-fix" | "--outcome" | "--published" | "--runner-host" | "--command" => {
                let value = rest.next()?.clone();
                match flag.as_str() {
                    "--base" => parsed.base = value,
                    "--head" => parsed.head = value,
                    "--dry" => parsed.dry = value == "1",
                    "--safe-push" => parsed.safe_push = value,
                    "--red-fix" => parsed.red_fix = value,
                    "--outcome" => parsed.outcome = Some(PathBuf::from(value)),
                    "--published" => parsed.published = value,
                    "--runner-host" => parsed.runner_host = value,
                    _ => parsed.commands.push(value),
                }
            }
            _ => return None,
        }
    }
    (!parsed.head.is_empty()).then_some(parsed)
}

#[derive(Clone)]
struct Environment {
    capacity: String,
    offload_host: String,
    timeout: Option<u64>,
    runner: Option<String>,
}

impl Environment {
    fn load() -> Self {
        let text = |name: &str| std::env::var(name).unwrap_or_default();
        Self {
            capacity: text("SAFE_PUSH_LAND_CAPACITY"),
            offload_host: text("SAFE_PUSH_LAND_OFFLOAD_HOST"),
            timeout: text("SAFE_PUSH_LAND_CHECK_TIMEOUT").parse().ok(),
            runner: std::env::var("SAFE_PUSH_LAND_RUNNER").ok(),
        }
    }
}

#[derive(Clone)]
struct Outcome(Option<PathBuf>);

impl Outcome {
    fn record(&self, fields: &[&str]) {
        let Some(path) = &self.0 else { return };
        let line = fields.iter().map(|field| field.replace(['\t', '\n'], " ")).collect::<Vec<_>>().join("\t");
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{line}");
        }
    }
}

#[derive(Clone)]
struct Shared {
    root: PathBuf,
    arguments: Arguments,
    environment: Environment,
    config: Config,
    outcome: Outcome,
    cancel: watch::Receiver<bool>,
    workflow: String,
    trees: Trees,
}

#[derive(Clone, Default)]
struct Trees {
    base: String,
    head: String,
}

impl Trees {
    fn of(root: &Path, base: &str, head: &str) -> Self {
        let tree = |revision: &str| {
            if revision.is_empty() {
                return String::new();
            }
            std::process::Command::new("git")
                .args(["rev-parse", "--verify", "--quiet", &format!("{revision}^{{tree}}")])
                .current_dir(root)
                .stderr(Stdio::null())
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
                .unwrap_or_default()
        };
        Self { base: tree(base), head: tree(head) }
    }
}

struct Ran {
    status: i32,
    seconds: u64,
    exceeded: bool,
    log: PathBuf,
}

impl Ran {
    fn timing(&self) -> Timing {
        if self.exceeded { Timing::Exceeded(self.seconds) } else { Timing::Finished(self.seconds) }
    }
}

struct Checkout {
    root: PathBuf,
    back: Vec<String>,
}

fn git_in(root: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git").args(arguments).current_dir(root).stdin(Stdio::null()).output().map_err(|error| format!("git {}: {error}", arguments.join(" ")))?;
    if !output.status.success() {
        return Err(format!("git {} failed: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

impl Checkout {
    fn switch(root: &Path, base: &str) -> Result<Self, String> {
        let back = match git_in(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
            Ok(branch) if !branch.is_empty() => vec![branch],
            _ => vec!["--detach".to_owned(), git_in(root, &["rev-parse", "HEAD"])?],
        };
        git_in(root, &["checkout", "-q", "--force", "--detach", base])?;
        Ok(Self { root: root.to_owned(), back })
    }

    fn restore(self) -> Result<(), String> {
        let mut arguments = vec!["checkout", "-q", "--force"];
        arguments.extend(self.back.iter().map(String::as_str));
        git_in(&self.root, &arguments).map(drop)
    }
}

const ERROR_WORDS: &[&str] = &["error", "not found", "fail", "panicked", "denied", "no such"];
const FIRST_ERROR_BOUND: usize = 300;

fn first_error(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
    let line = lines.iter().find(|line| {
        let lower = line.to_lowercase();
        ERROR_WORDS.iter().any(|word| lower.contains(word))
    }).or_else(|| lines.last())?;
    let mut cut = line.len().min(FIRST_ERROR_BOUND);
    while !line.is_char_boundary(cut) {
        cut -= 1;
    }
    Some(line[..cut].to_owned())
}

fn host_name(class: &str) -> String {
    if class != "local" {
        return class.to_owned();
    }
    fs::read_to_string("/proc/sys/kernel/hostname").map_or_else(|_| class.to_owned(), |name| name.trim().to_owned())
}

fn exit_code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status.code().or_else(|| status.signal().map(|signal| 128 + signal)).unwrap_or(1)
}

fn say(text: &str) {
    println!("{text}");
}

fn warn(text: &str) {
    eprintln!("{text}");
}

fn toplevel() -> Option<PathBuf> {
    let output = std::process::Command::new("git").args(["rev-parse", "--show-toplevel"]).output().ok()?;
    output.status.success().then(|| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim_end()))
}

fn shell_deadline(seconds: u64, argv: &[String]) -> Vec<String> {
    let mut command = vec![
        "bash".to_owned(),
        "-euo".to_owned(),
        "pipefail".to_owned(),
        "-c".to_owned(),
        r#"lease=-; [ -z "${AGENT_CAPACITY_LEASE:-}" ] || lease="${XDG_RUNTIME_DIR:-/run/user/$UID}/agent-capacity-v1/leases/$AGENT_CAPACITY_LEASE.json"; exec "$0" capacity deadline "$lease" "$@""#.to_owned(),
        program(),
        seconds.to_string(),
        "--".to_owned(),
    ];
    command.extend(argv.iter().cloned());
    command
}

fn under_capacity(capacity: &str, argv: Vec<String>) -> Vec<String> {
    if capacity.is_empty() {
        return argv;
    }
    let mut command = vec!["bash".to_owned(), "-euo".to_owned(), "pipefail".to_owned(), "-c".to_owned(), format!("exec {capacity} \"$@\""), "safe-push-capacity".to_owned()];
    command.extend(argv);
    command
}

fn log_directory() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state")))?;
    let directory = state.join("safe-push");
    fs::create_dir_all(&directory).ok()?;
    fs::canonicalize(directory).ok()
}

fn prune_logs(directory: &Path) {
    let Ok(entries) = fs::read_dir(directory) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !(name.starts_with("land-check-") && name.ends_with(".log")) {
            continue;
        }
        let old = entry.metadata().and_then(|meta| meta.modified()).ok().and_then(|time| SystemTime::now().duration_since(time).ok()).is_some_and(|age| age > LOG_KEEP);
        if old {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn new_log() -> Option<PathBuf> {
    let directory = log_directory()?;
    prune_logs(&directory);
    let file = tempfile::Builder::new().prefix("land-check-").suffix(".log").rand_bytes(10).tempfile_in(&directory).ok()?;
    file.keep().ok().map(|(_, path)| path)
}

enum Sink {
    Out,
    Err,
}

async fn tee(mut source: impl AsyncRead + Unpin, log: PathBuf, sink: Sink) {
    let Ok(mut file) = OpenOptions::new().append(true).open(&log) else { return };
    let mut buffer = [0u8; 8192];
    while let Ok(count) = source.read(&mut buffer).await {
        if count == 0 {
            break;
        }
        let _ = file.write_all(&buffer[..count]);
        match sink {
            Sink::Out => {
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(&buffer[..count]);
                let _ = out.flush();
            }
            Sink::Err => {
                let mut out = std::io::stderr().lock();
                let _ = out.write_all(&buffer[..count]);
                let _ = out.flush();
            }
        }
    }
}

impl Shared {
    async fn wait(&self, child: &mut Child) -> i32 {
        let mut cancel = self.cancel.clone();
        tokio::select! {
            status = child.wait() => status.map_or(1, exit_code),
            Ok(()) = cancel.changed() => {
                if let Some(pid) = child.id().and_then(|pid| i32::try_from(pid).ok()).and_then(Pid::from_raw) {
                    let _ = kill_process(pid, Signal::Term);
                }
                child.wait().await.map_or(1, exit_code)
            }
        }
    }

    fn offload_host_for(&self, command: &str) -> Option<String> {
        if !self.environment.offload_host.is_empty() {
            return Some(self.environment.offload_host.clone());
        }
        (!self.arguments.runner_host.is_empty() && self.config.local_reason(command).is_none()).then(|| self.arguments.runner_host.clone())
    }

    fn cache_class(&self, command: &str) -> String {
        if !self.environment.offload_host.is_empty() {
            return self.environment.offload_host.clone();
        }
        if !self.workflow.is_empty() && self.config.off_actions_reason(command).is_none() {
            return "actions".to_owned();
        }
        self.offload_host_for(command).unwrap_or_else(|| "local".to_owned())
    }

    async fn north(&self, arguments: &[&str]) -> Option<std::process::Output> {
        Command::new(program()).args(arguments).current_dir(&self.root).stdin(Stdio::null()).output().await.ok()
    }

    async fn cache_reused(&self, command: &str) -> bool {
        let head = &self.arguments.head;
        let root = self.root.to_string_lossy().into_owned();
        let class = self.cache_class(command);
        let Some(output) = self.north(&["land-cache", "hit", "--repo", &root, "--head", head, "--command", command, "--host-class", &class]).await else { return false };
        let line = String::from_utf8_lossy(&output.stdout).trim_end().to_owned();
        if output.status.success() && !line.is_empty() {
            say(&format!("safe-push: {line}"));
            return true;
        }
        false
    }

    async fn cache_record(&self, status: i32, commands: &[String]) {
        let root = self.root.to_string_lossy().into_owned();
        let status = status.to_string();
        for command in commands {
            let class = self.cache_class(command);
            let _ = self.north(&["land-cache", "record", "--repo", &root, "--head", &self.arguments.head, "--command", command, "--host-class", &class, "--status", &status]).await;
        }
    }

    async fn spawn_check(&self, command: &str, leased: bool, tree: &str, timeout: u64) -> Result<Ran, i32> {
        let environment = &self.environment;
        let argv = if let Some(host) = self.offload_host_for(command) {
            if !environment.offload_host.is_empty() && self.config.local_reason(command).is_some() {
                warn(&format!("safe-push: land check {command} needs this host, which is in game-reserve; NOT pushed."));
                return Err(1);
            }
            let offload = [program(), "capacity".into(), "offload".into(), "run".into(), "--host".into(), host.clone(), "--base".into(), self.arguments.base.clone(), "--head".into(), tree.to_owned(), "--timeout".into(), timeout.to_string(), "--".into(), command.to_owned()];
            let capacity = if environment.offload_host.is_empty() { "" } else { environment.capacity.as_str() };
            let suffix = if environment.offload_host.is_empty() { "" } else { " while this host is in game-reserve" };
            say(&format!("safe-push: offloading to {host}{suffix}"));
            under_capacity(capacity, shell_deadline(timeout + OFFLOAD_GRACE, &offload))
        } else {
            let local = ["bash".to_owned(), "-euo".to_owned(), "pipefail".to_owned(), "-c".to_owned(), command.to_owned()];
            let capacity = if leased { "" } else { environment.capacity.as_str() };
            under_capacity(capacity, shell_deadline(timeout, &local))
        };
        let Some(log) = new_log() else {
            warn("safe-push: could not create a land check log; NOT pushed.");
            return Err(1);
        };
        let started = Instant::now();
        let mut child = match Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(&self.root)
            .env("SAFE_PUSH_LAND_BASE", &self.arguments.base)
            .env("SAFE_PUSH_LAND_HEAD", &self.arguments.head)
            .env("SAFE_PUSH_LAND_REF", &self.arguments.published)
            .env("SAFE_PUSH_LANDING", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                warn(&format!("safe-push: land check {command}: {error}; NOT pushed."));
                return Err(1);
            }
        };
        let mut readers: Vec<_> = [
            child.stdout.take().map(|pipe| tokio::spawn(tee(pipe, log.clone(), Sink::Out))),
            child.stderr.take().map(|pipe| tokio::spawn(tee(pipe, log.clone(), Sink::Err))),
        ]
        .into_iter()
        .flatten()
        .collect();
        let status = self.wait(&mut child).await;
        let drained = tokio::time::timeout(REAP, async {
            for reader in &mut readers {
                let _ = reader.await;
            }
        })
        .await;
        if drained.is_err() {
            readers.iter().for_each(tokio::task::JoinHandle::abort);
        }
        let seconds = started.elapsed().as_secs();
        let exceeded = status == TIMED_OUT || (status == KILLED && seconds >= timeout);
        Ok(Ran { status, seconds, exceeded, log })
    }

    async fn run_check(self: Arc<Self>, command: String, leased: bool) -> i32 {
        self.outcome.record(&["ran"]);
        if !self.workflow.is_empty() && self.offload_host_for(&command).is_none() && let Some(reason) = self.config.off_actions_reason(&command) {
            say(&format!("safe-push: land check runs on this host because it {reason}"));
        }
        let timings = Timings::at(Timings::default_root());
        let class = self.cache_class(&command);
        let deadline = self.environment.timeout.map_or_else(|| timings.deadline(&command, &class), Deadline::configured);
        let timeout = deadline.seconds;
        let label = if self.config.is_inferred(&command) { " (inferred)" } else { "" };
        say(&format!("safe-push: land check{label}: {command} ({deadline})"));
        let ran = match self.spawn_check(&command, leased, &self.arguments.head, timeout).await {
            Ok(ran) => ran,
            Err(status) => return status,
        };
        let _ = timings.record(&self.trees.head, &command, &class, ran.timing());
        if ran.exceeded {
            return self.judge_hang(&command, timeout, timings.base(&self.trees.base, &command, &class, timeout), &ran.log).await;
        }
        let commands = [command.clone()];
        if ran.status != 0 {
            return self.judge_failure(&command, leased, timeout, &ran).await;
        }
        let _ = timings.passed(&command, &class, ran.seconds);
        self.cache_record(0, &commands).await;
        let _ = fs::remove_file(&ran.log);
        0
    }

    async fn judge_failure(&self, command: &str, leased: bool, timeout: u64, ran: &Ran) -> i32 {
        let (status, log) = (ran.status, ran.log.display());
        if status != INFRASTRUCTURE && let Some(first) = self.base_failure(command, leased, timeout).await {
            let tree = self.trees.base.get(..12).unwrap_or(&self.trees.base);
            let detail = format!("{BROKEN_BASE}{command} on {}: {first}", host_name(&self.cache_class(command)));
            warn(&format!("safe-push: land check {command} exited {status} on this lane and also failed on base tree {tree}, so the failure is the check environment, not this lane; log: {log}; NOT pushed."));
            self.outcome.record(&["skip", &detail]);
            self.outcome.record(&["event", "land check", "failed", &format!("{detail}; log: {log}")]);
            return INFRASTRUCTURE;
        }
        warn(&format!("safe-push: land check {command} exited {status}; log: {log}; NOT pushed."));
        self.cache_record(status, &[command.to_owned()]).await;
        self.outcome.record(&["event", "land check", "failed", &format!("{command}; log: {log}")]);
        status
    }

    async fn base_failure(&self, command: &str, leased: bool, timeout: u64) -> Option<String> {
        let round = std::env::var(ROUND).unwrap_or_default();
        let judged = !round.is_empty() && !*self.cancel.borrow() && !self.trees.base.is_empty() && self.trees.base != self.trees.head && Config::load(&self.root, &self.arguments.base).runs(command);
        if !judged {
            return None;
        }
        let class = self.cache_class(command);
        let timings = Timings::at(Timings::default_root());
        let tree = self.trees.base.get(..12).unwrap_or(&self.trees.base);
        let result = if let Some(result) = timings.result(&self.trees.base, command, &class, &round) {
            say(&format!("safe-push: base tree {tree} already ran land check {command} this round"));
            result
        } else {
            say(&format!("safe-push: land check failed; rerunning it on base tree {tree}: {command}"));
            let result = self.run_on_base(command, leased, timeout).await;
            let _ = timings.record_result(&self.trees.base, command, &class, &round, &result);
            result
        };
        match result {
            BaseResult::Failed(first) => Some(first),
            BaseResult::Passed => None,
        }
    }

    async fn run_on_base(&self, command: &str, leased: bool, timeout: u64) -> BaseResult {
        let base = self.arguments.base.clone();
        let checkout = if self.offload_host_for(command).is_some() {
            None
        } else {
            match Checkout::switch(&self.root, &base) {
                Ok(checkout) => Some(checkout),
                Err(error) => return BaseResult::Failed(error),
            }
        };
        let ran = self.spawn_check(command, leased, &base, timeout).await;
        if let Some(checkout) = checkout
            && let Err(error) = checkout.restore()
        {
            warn(&format!("safe-push: {error}"));
        }
        let Ok(ran) = ran else { return BaseResult::Failed(format!("land check {command} could not start on the base tree")) };
        let _ = Timings::at(Timings::default_root()).record(&self.trees.base, command, &self.cache_class(command), ran.timing());
        if ran.status == 0 {
            let _ = fs::remove_file(&ran.log);
            return BaseResult::Passed;
        }
        let first = first_error(&fs::read_to_string(&ran.log).unwrap_or_default()).unwrap_or_else(|| format!("exited {}", ran.status));
        say(&format!("safe-push: land check {command} exited {} on the base tree too; log: {}", ran.status, ran.log.display()));
        BaseResult::Failed(first)
    }

    async fn judge_hang(&self, command: &str, timeout: u64, base: Base, log: &Path) -> i32 {
        let tree = self.trees.base.get(..12).unwrap_or(&self.trees.base);
        let base = match base {
            Base::Within(seconds) => {
                warn(&format!("safe-push: land check {command} hung: it ran past its {timeout} s deadline on this lane, while base tree {tree} finished it in {seconds} s; log: {}; NOT pushed.", log.display()));
                self.cache_record(TIMED_OUT, &[command.to_owned()]).await;
                self.outcome.record(&["event", "land check", "failed", &format!("{command} hung past its {timeout} s deadline; log: {}", log.display())]);
                return TIMED_OUT;
            }
            Base::Exceeded => "also ran past it",
            Base::Untimed => "has never been timed",
        };
        let detail = format!("land check {command} ran past its {timeout} s deadline and base tree {tree} {base}, so the hang is infrastructure, not this lane");
        warn(&format!("safe-push: {detail}; log: {}; NOT pushed.", log.display()));
        self.outcome.record(&["skip", &detail]);
        self.outcome.record(&["event", "land check", "failed", &format!("{detail}; log: {}", log.display())]);
        INFRASTRUCTURE
    }
}

fn first_failure(failed: &mut i32, status: i32) {
    if *failed == 0 || (*failed == INFRASTRUCTURE && status != 0) {
        *failed = status;
    }
}

impl Shared {
    async fn run_local(self: &Arc<Self>, commands: &[String]) -> i32 {
        if self.environment.capacity.is_empty() || self.arguments.leased {
            let mut failed = 0;
            for command in commands {
                let status = Arc::clone(self).run_check(command.clone(), self.arguments.leased).await;
                first_failure(&mut failed, status);
            }
            return failed;
        }
        let Ok(executable) = std::env::current_exe() else { return 1 };
        let mut argv = vec![executable.to_string_lossy().into_owned(), "land".into(), "checks".into(), "--leased".into(), "--base".into(), self.arguments.base.clone(), "--head".into(), self.arguments.head.clone(), "--published".into(), self.arguments.published.clone(), "--runner-host".into(), self.arguments.runner_host.clone()];
        if let Some(outcome) = &self.outcome.0 {
            argv.extend(["--outcome".to_owned(), outcome.to_string_lossy().into_owned()]);
        }
        for command in commands {
            argv.extend(["--command".to_owned(), command.clone()]);
        }
        let argv = under_capacity(&self.environment.capacity, argv);
        let mut child = match Command::new(&argv[0]).args(&argv[1..]).current_dir(&self.root).stdin(Stdio::null()).kill_on_drop(true).spawn() {
            Ok(child) => child,
            Err(error) => {
                warn(&format!("safe-push: land check capacity: {error}; NOT pushed."));
                return 1;
            }
        };
        self.wait(&mut child).await
    }

    async fn run_list(self: &Arc<Self>, commands: &[String]) -> i32 {
        let mut offloaded = JoinSet::new();
        let mut local = Vec::new();
        for (index, command) in commands.iter().enumerate() {
            if self.offload_host_for(command).is_some() {
                let shared = Arc::clone(self);
                let command = command.clone();
                offloaded.spawn(async move { (index, shared.run_check(command, false).await) });
            } else {
                local.push(command.clone());
            }
        }
        let mut failed = 0;
        if !local.is_empty() {
            let status = self.run_local(&local).await;
            first_failure(&mut failed, status);
        }
        let mut results = Vec::new();
        while let Some(joined) = offloaded.join_next().await {
            results.push(joined.unwrap_or((usize::MAX, 1)));
        }
        results.sort_unstable();
        for (_, status) in results {
            first_failure(&mut failed, status);
        }
        failed
    }

    async fn scan(&self, skipped: &str) -> bool {
        let passed = Command::new(&self.arguments.safe_push)
            .args(["--publication-scan", &self.arguments.base, &self.arguments.head])
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .status()
            .await
            .is_ok_and(|status| status.success());
        if !passed {
            self.outcome.record(&["skip", skipped]);
        }
        passed
    }

    async fn git_push(&self, refspec: &str) -> bool {
        Command::new("git")
            .args(["push", "-q", "--no-verify", "origin", refspec])
            .current_dir(&self.root)
            .env("SAFE_PUSH_ACTIVE", "1")
            .stdin(Stdio::null())
            .status()
            .await
            .is_ok_and(|status| status.success())
    }

    async fn red_main_check(&self, log: &Path) {
        let lane = std::process::Command::new("git").args(["symbolic-ref", "--quiet", "--short", "HEAD"]).current_dir(&self.root).output().map(|output| String::from_utf8_lossy(&output.stdout).trim_end().to_owned()).unwrap_or_default();
        let status = Command::new(program())
            .args(["land-red-check", "--main", &self.arguments.base, "--log"])
            .arg(log)
            .args(["--lane", &lane])
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .status()
            .await
            .map_or(1, exit_code);
        if status == RED_MAIN {
            self.outcome.record(&["red-main", &self.arguments.base]);
        }
    }

    async fn execute(self: Arc<Self>, commands: Vec<String>) -> i32 {
        let arguments = &self.arguments;
        let mut here = Vec::new();
        let mut on_actions = Vec::new();
        let land_runner = self.environment.runner.clone().unwrap_or_else(|| self.config.runner.clone().unwrap_or_default());
        for command in commands {
            if self.cache_reused(&command).await {
                continue;
            }
            let portable = self.config.off_actions_reason(&command).is_none();
            if !self.workflow.is_empty() && land_runner.is_empty() && self.environment.offload_host.is_empty() && portable {
                on_actions.push(command);
            } else {
                here.push(command);
            }
        }
        let timeout = self.environment.timeout.unwrap_or(check_timings::CEILING);
        let mut actions = None;
        let mut actions_log = None;
        if !on_actions.is_empty() {
            if !self.scan("skipped: the lane cannot be published to GitHub Actions until the publication scan passes").await {
                return 1;
            }
            say(&format!("safe-push: running {} land check(s) on GitHub Actions ({})", on_actions.len(), self.workflow));
            let Ok(log) = tempfile::Builder::new().tempfile().and_then(|file| file.keep().map_err(|error| error.error)) else { return 1 };
            let log = log.1;
            let spawned = Command::new(program())
                .args(["land-actions", "--workflow", &self.workflow, "--base", &arguments.base, "--head", &arguments.head, "--timeout", &timeout.to_string(), "--"])
                .args(&on_actions)
                .current_dir(&self.root)
                .env("SAFE_PUSH_LAND_FAILED_LOG", &log)
                .stdin(Stdio::null())
                .kill_on_drop(true)
                .spawn();
            match spawned {
                Ok(child) => {
                    actions = Some(child);
                    actions_log = Some(log);
                }
                Err(error) => {
                    warn(&format!("safe-push: could not start north land-actions: {error}; NOT pushed."));
                    return 1;
                }
            }
        }
        let mut published = None;
        if !here.is_empty() && !arguments.dry && self.environment.offload_host.is_empty() && let Some(reference) = self.config.publish_ref(&arguments.head) {
            if !self.scan("skipped: the lane cannot be published for them until the publication scan passes").await {
                return 1;
            }
            if !self.git_push(&format!("+{}:refs/heads/{reference}", arguments.head)).await {
                warn(&format!("safe-push: could not publish {} to origin {reference} for the land checks; NOT pushed.", arguments.head));
                return 1;
            }
            say(&format!("safe-push: published {} to origin {reference} for the land checks", &arguments.head[..arguments.head.len().min(12)]));
            published = Some(reference);
        }
        let mut failed = if here.is_empty() {
            0
        } else {
            let shared = Arc::new(self.with_published(published.clone().unwrap_or_default()));
            shared.run_list(&here).await
        };
        if let Some(reference) = &published && !self.git_push(&format!(":refs/heads/{reference}")).await {
            warn(&format!("safe-push: could not delete origin {reference}"));
        }
        if let (Some(mut child), Some(log)) = (actions, actions_log) {
            let status = self.wait(&mut child).await;
            if status == INFRASTRUCTURE {
                say(&format!("safe-push: GitHub Actions is unavailable (infrastructure, not a check failure); its {} land check(s) run on a build host", on_actions.len()));
                let fallback = Arc::new(self.with_runner_host(resolve_runner_host("build-host").await));
                let status = fallback.run_list(&on_actions).await;
                first_failure(&mut failed, status);
            } else if status != 0 {
                warn(&format!("safe-push: land checks on GitHub Actions exited {status}; NOT pushed."));
                self.outcome.record(&["event", "land checks on GitHub Actions", "failed", &format!("exit {status}")]);
                first_failure(&mut failed, status);
                if arguments.red_fix.is_empty() {
                    self.red_main_check(&log).await;
                }
            }
            self.cache_record(status, &on_actions).await;
            let _ = fs::remove_file(&log);
        }
        failed
    }
}

impl Shared {
    fn with_published(&self, published: String) -> Self {
        let mut shared = self.clone();
        shared.arguments.published = published;
        shared
    }

    fn with_runner_host(&self, host: String) -> Self {
        let mut shared = self.clone();
        shared.arguments.runner_host = host;
        shared
    }
}

async fn resolve_runner_host(value: &str) -> String {
    if value != "build-host" {
        return value.to_owned();
    }
    let chosen = Command::new(program()).args(["capacity", "offload", "choose", "--any"]).stdin(Stdio::null()).stderr(Stdio::inherit()).output().await;
    match chosen {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout).trim_end().to_owned(),
        _ => {
            warn("safe-push: no build host has room; the land checks run on this host under the land-check capacity lease");
            String::new()
        }
    }
}

async fn runner_host(environment: &Environment, config: &Config) -> String {
    let land_runner = environment.runner.clone().unwrap_or_else(|| config.runner.clone().unwrap_or_default());
    if land_runner.is_empty() { String::new() } else { resolve_runner_host(&land_runner).await }
}

async fn forward_signals(cancel: watch::Sender<bool>) {
    let (Ok(mut terminate), Ok(mut interrupt), Ok(mut hangup)) = (signal(SignalKind::terminate()), signal(SignalKind::interrupt()), signal(SignalKind::hangup())) else { return };
    tokio::select! {
        _ = terminate.recv() => {}
        _ = interrupt.recv() => {}
        _ = hangup.recv() => {}
    }
    let _ = cancel.send(true);
    tokio::time::sleep(CANCEL_GRACE).await;
    std::process::exit(CANCELLED);
}

fn portable(arguments: &Arguments) -> u8 {
    let Some(root) = toplevel() else { return 1 };
    let config = Config::load(&root, &arguments.head);
    let Ok(commands) = config.commands(&root, &arguments.base, &arguments.head) else { return 1 };
    let portable = config.workflow.is_empty() && !commands.is_empty() && commands.iter().all(|command| config.local_reason(command).is_none());
    u8::from(!portable)
}

fn any(arguments: &Arguments) -> u8 {
    let Some(root) = toplevel() else { return 1 };
    u8::from(!Config::load(&root, &arguments.head).has_checks())
}

async fn orchestrate(arguments: Arguments) -> i32 {
    let Some(root) = toplevel() else {
        warn("safe-push: not in a git repo");
        return 1;
    };
    let config = Config::load(&root, &arguments.head);
    let config = if arguments.after { config.after_set() } else { config };
    let (cancel_sender, cancel) = watch::channel(false);
    tokio::spawn(forward_signals(cancel_sender));
    let environment = Environment::load();
    let commands = if arguments.leased {
        arguments.commands.clone()
    } else {
        match config.commands(&root, &arguments.base, &arguments.head) {
            Ok(commands) => commands,
            Err(message) => {
                warn(&format!("safe-push: {message}; NOT pushed."));
                return 1;
            }
        }
    };
    let mut arguments = arguments;
    if !arguments.leased && !commands.is_empty() && commands.iter().all(|command| config.is_inferred(command)) {
        say(&format!("safe-push: no land.check lines in .safe-push; running the {} land check(s) inferred from the tree", commands.len()));
    }
    if !arguments.leased {
        arguments.runner_host = runner_host(&environment, &config).await;
    }
    let trees = Trees::of(&root, &arguments.base, &arguments.head);
    let shared = Arc::new(Shared {
        trees,
        workflow: config.workflow.clone(),
        outcome: Outcome(arguments.outcome.clone()),
        root,
        arguments,
        environment,
        config,
        cancel,
    });
    if shared.arguments.leased {
        return shared.run_list(&commands).await;
    }
    shared.execute(commands).await
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(parsed) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    if parsed.portable {
        return portable(&parsed);
    }
    if parsed.any {
        return any(&parsed);
    }
    let code = block_on(orchestrate(parsed)).unwrap_or_else(|error| {
        eprintln!("north land checks: {error}");
        1
    });
    u8::try_from(code).unwrap_or(1)
}

fn block_on(work: impl Future<Output = i32>) -> std::io::Result<i32> {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        return Ok(tokio::task::block_in_place(|| handle.block_on(work)));
    }
    Ok(tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(work))
}

pub fn fails_first(root: &Path, base: &str, head: &str) -> Result<(), String> {
    let config = Config::load(root, head);
    let commands = config.added_since(&Config::load(root, base)).commands(root, base, head)?;
    if commands.is_empty() {
        return Ok(());
    }
    say(&format!("safe-push: running the {} land check(s) this lane adds before queueing it", commands.len()));
    let arguments = Arguments { base: base.to_owned(), head: head.to_owned(), ..Arguments::default() };
    let status = block_on(run_added(root.to_owned(), arguments, config, commands)).map_err(|error| error.to_string())?;
    if status == 0 { Ok(()) } else { Err(format!("a land check this lane adds exited {status}")) }
}

async fn run_added(root: PathBuf, mut arguments: Arguments, config: Config, commands: Vec<String>) -> i32 {
    let mut environment = Environment::load();
    if environment.capacity.is_empty() {
        environment.capacity = config.capacity.clone().unwrap_or_default();
    }
    arguments.runner_host = runner_host(&environment, &config).await;
    let (_cancel_sender, cancel) = watch::channel(false);
    let trees = Trees::of(&root, &arguments.base, &arguments.head);
    let shared = Arc::new(Shared { root, arguments, environment, config, outcome: Outcome(None), cancel, workflow: String::new(), trees });
    let mut pending = Vec::new();
    for command in commands {
        if !shared.cache_reused(&command).await {
            pending.push(command);
        }
    }
    if pending.is_empty() { 0 } else { shared.run_list(&pending).await }
}
