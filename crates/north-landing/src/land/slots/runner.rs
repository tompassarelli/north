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

use super::affected::{self, CHANGED_ENV, CPUS_ENV, PACKAGES_ENV, Scope};
use super::it_modules::{self, IT_ENV};
use super::clippy_first;
use super::deps;
use super::shared_build;
use super::select::{Config, OFFLOAD_SLOTS_BUSY, Placement, changed, path_class};
use crate::landing::check_timings::outcomes::{Outcome as CheckOutcome, Record, Records};
use fail_fast::Plan;

mod fail_fast;
mod queue_clippy;
mod timeout_row;
mod verdict;
pub use queue_clippy::refuse_clippy;
use crate::land::after::follow::program;
use crate::land::checks::{BROKEN_BASE, ROUND};
use crate::land::telemetry::epoch;
use crate::land::trace::{self, Category, Span};
use crate::land::trace::check_log::{self, Line, Mark};
use crate::landing::check_timings::{self, Base, BaseResult, Deadline, Timing, Timings};
use crate::landing::land_test_selection::SELECT_PATHS_ENV;

const USAGE: &str = "usage: north land checks --base BASE --head HEAD [--dry 0|1] [--safe-push PATH] [--red-fix ISSUE] [--outcome FILE] [--destination BRANCH] [--event-log FILE --wait-event LINE]\n       north land checks --portable --base BASE --head HEAD\n       north land checks --any --head HEAD\n       north land checks --after --base BASE --head HEAD [--tree TREE] [--outcome FILE]\n       north land checks --replay [--records FILE] [--since EPOCH] [--until EPOCH]";
const OFFLOAD_GRACE: u64 = 900;
const REAP: Duration = Duration::from_secs(5);
const LOG_KEEP: Duration = Duration::from_secs(14 * 24 * 3600);
const INFRASTRUCTURE: i32 = 75;
const TIMED_OUT: i32 = 124;
const KILLED: i32 = 137;
const RED_MAIN: i32 = 3;
const CANCELLED: i32 = 130;
const CANCEL_GRACE: Duration = Duration::from_secs(15);
const ACTIONS_CLASS: &str = "actions";

#[derive(Clone, Default)]
struct Arguments {
    base: String,
    head: String,
    tree: String,
    dry: bool,
    safe_push: String,
    red_fix: String,
    outcome: Option<PathBuf>,
    destination: Option<String>,
    event_log: Option<PathBuf>,
    wait_event: String,
    portable: bool,
    any: bool,
    after: bool,
    leased: bool,
    replay: bool,
    records: Option<PathBuf>,
    since: u64,
    until: u64,
    published: String,
    runner_host: String,
    commands: Vec<String>,
    span: String,
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
            "--replay" => parsed.replay = true,
            "--base" | "--head" | "--tree" | "--dry" | "--safe-push" | "--red-fix" | "--outcome" | "--destination" | "--event-log" | "--wait-event" | "--published" | "--runner-host" | "--command" | "--records" | "--since" | "--until" => {
                let value = rest.next()?.clone();
                match flag.as_str() {
                    "--base" => parsed.base = value,
                    "--head" => parsed.head = value,
                    "--tree" => parsed.tree = value,
                    "--dry" => parsed.dry = value == "1",
                    "--safe-push" => parsed.safe_push = value,
                    "--red-fix" => parsed.red_fix = value,
                    "--outcome" => parsed.outcome = Some(PathBuf::from(value)),
                    "--destination" => parsed.destination = Some(value),
                    "--event-log" => parsed.event_log = Some(PathBuf::from(value)),
                    "--wait-event" => parsed.wait_event = value,
                    "--published" => parsed.published = value,
                    "--runner-host" => parsed.runner_host = value,
                    "--records" => parsed.records = Some(PathBuf::from(value)),
                    "--since" => parsed.since = value.parse().ok()?,
                    "--until" => parsed.until = value.parse().ok()?,
                    _ => parsed.commands.push(value),
                }
            }
            _ => return None,
        }
    }
    (parsed.replay || !parsed.head.is_empty()).then_some(parsed)
}

impl Arguments {
    fn tree(&self) -> &str {
        if self.tree.is_empty() { &self.head } else { &self.tree }
    }

    fn pinned(&self) -> bool {
        self.tree() != self.head
    }
}

#[derive(Clone)]
struct Environment {
    capacity: String,
    offload_host: String,
    timeout: Option<u64>,
    runner: Option<String>,
}

impl Environment {
    fn local() -> Self {
        Self { capacity: String::new(), offload_host: String::new(), timeout: std::env::var("SAFE_PUSH_LAND_CHECK_TIMEOUT").ok().and_then(|value| value.parse().ok()), runner: None }
    }

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
    free_slots: Option<u32>,
    actions_down: bool,
    paths: String,
    fail_fast: bool,
    scope: Option<Scope>,
    it: Option<String>,
    selected_paths: Option<String>,
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
    started_at: f64,
    ended_at: f64,
    marks: Vec<Mark>,
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

async fn tee(mut source: impl AsyncRead + Unpin, log: PathBuf, sink: Sink) -> Vec<Mark> {
    let mut marks = Vec::new();
    let Ok(mut file) = OpenOptions::new().append(true).open(&log) else { return marks };
    let mut buffer = [0u8; 8192];
    let mut pending = Vec::new();
    while let Ok(count) = source.read(&mut buffer).await {
        if count == 0 {
            break;
        }
        let _ = file.write_all(&buffer[..count]);
        pending.extend_from_slice(&buffer[..count]);
        while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
            let line = String::from_utf8_lossy(&pending[..end]).into_owned();
            pending.drain(..=end);
            if check_log::is_marker(&line) {
                marks.push(Mark { at: epoch(), line });
            }
        }
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
    marks
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

    fn scope_environment(&self) -> Vec<(&'static str, String)> {
        let mut environment = Vec::new();
        if let Some(scope) = &self.scope {
            environment.push((PACKAGES_ENV, scope.arguments()));
            environment.push((CHANGED_ENV, scope.changed_arguments()));
            environment.extend(scope.cpus().map(|cpus| (CPUS_ENV, cpus.to_string())));
        }
        environment.extend(self.it.clone().map(|filters| (IT_ENV, filters)));
        environment.extend(self.selected_paths.clone().map(|paths| (SELECT_PATHS_ENV, paths)));
        environment
    }

    fn land_runner(&self) -> String {
        self.environment.runner.clone().unwrap_or_else(|| self.config.runner.clone().unwrap_or_default())
    }

    fn placement(&self) -> Placement {
        Placement { workflow: !self.workflow.is_empty() && !self.actions_down, runner: !self.land_runner().is_empty(), reserve: !self.environment.offload_host.is_empty(), free_slots: self.free_slots }
    }

    fn portable(&self, command: &str) -> bool {
        self.config.off_actions_reason(command).is_none()
    }

    fn overflows(&self, command: &str, status: i32) -> bool {
        self.placement().overflows(status, self.offload_host_for(command).is_some(), self.portable(command))
    }

    async fn count_free_slots(&self) -> Option<u32> {
        let host = &self.arguments.runner_host;
        if !self.placement().probes_slots() || host.is_empty() {
            return None;
        }
        let Some(output) = self.north(&["capacity", "offload", "free", "--host", host]).await else { return Some(0) };
        let free = if output.status.success() { String::from_utf8_lossy(&output.stdout).trim().parse().unwrap_or(0) } else { 0 };
        say(&format!("safe-push: {host} has {free} free land-check slot(s)"));
        Some(free)
    }

    fn cache_class(&self, command: &str) -> String {
        if !self.environment.offload_host.is_empty() {
            return self.environment.offload_host.clone();
        }
        if self.placement().on_actions(self.portable(command)) {
            return ACTIONS_CLASS.to_owned();
        }
        self.offload_host_for(command).unwrap_or_else(|| "local".to_owned())
    }

    async fn north(&self, arguments: &[&str]) -> Option<std::process::Output> {
        Command::new(program()).args(arguments).current_dir(&self.root).stdin(Stdio::null()).output().await.ok()
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
        let started_at = epoch();
        let mut child = match Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(&self.root)
            .envs(self.scope_environment())
            .env("SAFE_PUSH_LAND_BASE", &self.arguments.base)
            .env("SAFE_PUSH_LAND_HEAD", tree)
            .env("SAFE_PUSH_LAND_RUNTIME_HEAD", self.arguments.tree())
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
        let ended_at = epoch();
        let mut marks = Vec::new();
        let drained = tokio::time::timeout(REAP, async {
            for reader in &mut readers {
                if let Ok(read) = reader.await {
                    marks.extend(read);
                }
            }
        })
        .await;
        if drained.is_err() {
            readers.iter().for_each(tokio::task::JoinHandle::abort);
        }
        marks.sort_by(|left, right| left.at.total_cmp(&right.at));
        let seconds = started.elapsed().as_secs();
        let exceeded = status == TIMED_OUT || (status == KILLED && seconds >= timeout);
        Ok(Ran { status, seconds, exceeded, log, started_at, ended_at, marks })
    }

    fn span(&self, name: &str, category: Category, start: f64) -> Span {
        Span::new(&self.arguments.head, name, category, start, epoch()).under(&self.arguments.span).on(&trace::this_host())
    }

    fn trace_line(&self, command: &str, ran: &Ran, cpu: Option<f64>) {
        let category = Category::of_command(command);
        let host = self.offload_host_for(command).unwrap_or_else(trace::this_host);
        let head = &self.arguments.head;
        let line = Span::new(head, command, category, ran.started_at, ran.ended_at).under(&self.arguments.span).on(&host).cpu(cpu).with("cache", "miss").with("exit", ran.status.to_string());
        let mut spans = check_log::children(&Line { trace: head, id: &line.id, category, start: line.start, end: line.end, host: &host }, &ran.marks);
        spans.insert(0, line);
        trace::record(&spans);
    }

    fn remember(&self, command: &str, started: u64, status: i32, outcome: CheckOutcome, wall: u64, cpu: Option<f64>) {
        #[allow(clippy::cast_precision_loss)]
        let wall = wall as f64;
        let record = Record { started, landing: self.arguments.head.clone(), paths: self.paths.clone(), outcome, exit: status, wall, cpu, command: command.to_owned() };
        let _ = Records::at(Records::default_path()).append(&record);
    }

    fn cancelled(&self, command: &str, started: u64, status: i32, wall: u64, cpu: Option<f64>) -> i32 {
        say(&format!("safe-push: land check {command} cancelled: an earlier land check failed"));
        self.remember(command, started, status, CheckOutcome::Cancelled, wall, cpu);
        self.outcome.record(&["event", "land check", "cancelled", &format!("{command}: an earlier land check failed")]);
        0
    }

    async fn run_check(self: Arc<Self>, command: String, leased: bool) -> i32 {
        let started = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs());
        if self.fail_fast && *self.cancel.borrow() {
            return self.cancelled(&command, started, CANCELLED, 0, None);
        }
        self.outcome.record(&["ran"]);
        if !self.workflow.is_empty() && self.offload_host_for(&command).is_none() && let Some(reason) = self.config.off_actions_reason(&command) {
            say(&format!("safe-push: land check runs on this host because it {reason}"));
        }
        let timings = Timings::at(Timings::default_root());
        let class = self.cache_class(&command);
        let deadline = self.environment.timeout.or_else(|| self.config.deadline_seconds(&command)).map_or_else(|| timings.deadline(&command, &class), Deadline::configured);
        let timeout = deadline.seconds;
        let label = if self.config.is_inferred(&command) { " (inferred)" } else { "" };
        say(&format!("safe-push: land check{label}: {command} ({deadline})"));
        let measured = self.offload_host_for(&command).is_none();
        let before = children_cpu_seconds();
        let ran = match self.spawn_check(&command, leased, self.arguments.tree(), timeout).await {
            Ok(ran) => ran,
            Err(status) => return status,
        };
        if self.overflows(&command, ran.status) {
            say(&format!("safe-push: every build-host slot stayed busy, so land check {command} overflows to GitHub Actions"));
            let _ = fs::remove_file(&ran.log);
            return ran.status;
        }
        let cpu = before.zip(children_cpu_seconds()).filter(|_| measured).map(|(before, after)| after - before);
        self.trace_line(&command, &ran, cpu);
        if self.fail_fast && *self.cancel.borrow() {
            let _ = fs::remove_file(&ran.log);
            return self.cancelled(&command, started, ran.status, ran.seconds, cpu);
        }
        self.remember(&command, started, ran.status, CheckOutcome::of(ran.status), ran.seconds, cpu);
        let _ = timings.record(&self.trees.head, &command, &class, ran.timing());
        if ran.exceeded {
            let base = timings.base(&self.trees.base, &command, &class, timeout);
            if base == Base::Exceeded {
                let _ = timings.exceeded_on_base_too(&command, &class, timeout);
            }
            self.record_timeout(&command, timeout, started);
            return self.judge_hang(&command, timeout, base, &ran.log).await;
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

    fn record_timeout(&self, command: &str, deadline: u64, started: u64) {
        let finished = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(started, |since| since.as_secs());
        let (base, candidate, tree) = (&self.arguments.base, &self.arguments.head, &self.trees.head);
        timeout_row::record(&self.root, &timeout_row::Timeout { base, candidate, tree, command, deadline, started, finished });
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
            Base::Within(seconds) => match self.overloaded_host(command) {
                Some(host) => format!("finished it in {seconds} s while this host ran at load {:.1} over its {} cores", host.load, host.cores),
                None => {
                    warn(&format!("safe-push: land check {command} hung: it ran past its {timeout} s deadline on this lane, while base tree {tree} finished it in {seconds} s; log: {}; NOT pushed.", log.display()));
                    self.cache_record(TIMED_OUT, &[command.to_owned()]).await;
                    self.outcome.record(&["event", "land check", "failed", &format!("{command} hung past its {timeout} s deadline; log: {}", log.display())]);
                    return TIMED_OUT;
                }
            },
            Base::Exceeded => "also ran past it".to_owned(),
            Base::Untimed => "has never been timed".to_owned(),
        };
        let detail = format!("land check {command} ran past its {timeout} s deadline and base tree {tree} {base}, so the hang is infrastructure, not this lane");
        warn(&format!("safe-push: {detail}; log: {}; NOT pushed.", log.display()));
        self.outcome.record(&["skip", &detail]);
        self.outcome.record(&["event", "land check", "failed", &format!("{detail}; log: {}", log.display())]);
        INFRASTRUCTURE
    }

    fn overloaded_host(&self, command: &str) -> Option<HostLoad> {
        if self.offload_host_for(command).is_some() {
            return None;
        }
        HostLoad::read().filter(|host| host.load > host.cores as f64)
    }
}

struct HostLoad {
    load: f64,
    cores: usize,
}

impl HostLoad {
    fn read() -> Option<Self> {
        let path = std::env::var_os("AGENT_CAPACITY_LOADAVG").unwrap_or_else(|| "/proc/loadavg".into());
        let text = fs::read_to_string(path).ok()?;
        let mut averages = text.split_whitespace().map(|field| field.parse::<f64>().ok());
        let one = averages.next()??;
        let load = averages.next().flatten().map_or(one, |five| one.max(five));
        let configured = std::env::var("AGENT_CAPACITY_CORES").ok().and_then(|cores| cores.parse().ok()).filter(|cores| *cores > 0);
        let cores = configured.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from));
        Some(Self { load, cores })
    }
}

fn children_cpu_seconds() -> Option<f64> {
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
    let ticks: u64 = fields.get(13)?.parse::<u64>().ok()? + fields.get(14)?.parse::<u64>().ok()?;
    // proc(5) reports cutime and cstime in USER_HZ ticks, which Linux fixes at 100 per second.
    #[allow(clippy::cast_precision_loss)]
    Some(ticks as f64 / 100.0)
}

async fn forward_cancel(mut cancel: watch::Receiver<bool>, stop: Arc<watch::Sender<bool>>) {
    while cancel.changed().await.is_ok() {
        if *cancel.borrow() {
            stop.send_replace(true);
            return;
        }
    }
}

fn first_failure(failed: &mut i32, status: i32) {
    if *failed == 0 || (matches!(*failed, INFRASTRUCTURE | OFFLOAD_SLOTS_BUSY) && status != 0 && status != OFFLOAD_SLOTS_BUSY) {
        *failed = status;
    }
}

struct Listed {
    failed: i32,
    overflowed: Vec<String>,
}

impl Listed {
    fn status(&self) -> i32 {
        if self.failed == 0 && !self.overflowed.is_empty() { OFFLOAD_SLOTS_BUSY } else { self.failed }
    }
}

impl Shared {
    async fn run_local(self: &Arc<Self>, commands: &[String], plan: &Plan, expensive: &Arc<Self>, stop: &watch::Sender<bool>) -> i32 {
        if self.environment.capacity.is_empty() || self.arguments.leased {
            let mut failed = 0;
            for command in commands {
                let shared = if plan.is_expensive(command) { Arc::clone(expensive) } else { Arc::clone(self) };
                let status = shared.run_check(command.clone(), self.arguments.leased).await;
                if status != 0 {
                    stop.send_replace(true);
                }
                first_failure(&mut failed, status);
            }
            return failed;
        }
        let Ok(executable) = std::env::current_exe() else { return 1 };
        let mut argv = vec![executable.to_string_lossy().into_owned(), "land".into(), "checks".into(), "--leased".into(), "--base".into(), self.arguments.base.clone(), "--head".into(), self.arguments.head.clone(), "--published".into(), self.arguments.published.clone(), "--runner-host".into(), self.arguments.runner_host.clone()];
        if self.arguments.pinned() {
            argv.extend(["--tree".to_owned(), self.arguments.tree.clone()]);
        }
        if let Some(outcome) = &self.outcome.0 {
            argv.extend(["--outcome".to_owned(), outcome.to_string_lossy().into_owned()]);
        }
        for command in commands {
            argv.extend(["--command".to_owned(), command.clone()]);
        }
        let argv = under_capacity(&self.environment.capacity, argv);
        let lease = self.span("local checks under a capacity lease", Category::HostWait, epoch());
        let mut child = match Command::new(&argv[0]).args(&argv[1..]).current_dir(&self.root).envs(self.scope_environment()).env(trace::PARENT_ENV, &lease.id).stdin(Stdio::null()).kill_on_drop(true).spawn() {
            Ok(child) => child,
            Err(error) => {
                warn(&format!("safe-push: land check capacity: {error}; NOT pushed."));
                return 1;
            }
        };
        let status = self.wait(&mut child).await;
        trace::record(&[Span { end: epoch(), ..lease }]);
        status
    }

    async fn run_list(self: &Arc<Self>, commands: &[String]) -> Listed {
        let plan = Plan::of(commands, &self.paths, &Records::at(Records::default_path()).read());
        if !plan.expensive.is_empty() {
            say(&format!("safe-push: {} expensive land check(s) run in fail-fast order and are cancelled at the first failure: {}", plan.expensive.len(), plan.expensive.join("; ")));
        }
        let (stop, stopped) = watch::channel(false);
        let stop = Arc::new(stop);
        tokio::spawn(forward_cancel(self.cancel.clone(), Arc::clone(&stop)));
        let expensive = Arc::new(Self { cancel: stopped, fail_fast: true, ..(**self).clone() });
        let ordered: Vec<String> = plan.ordered().cloned().collect();
        let mut offloaded = JoinSet::new();
        let mut local = Vec::new();
        for (index, command) in ordered.iter().enumerate() {
            if self.offload_host_for(command).is_some() {
                let shared = if plan.is_expensive(command) { Arc::clone(&expensive) } else { Arc::clone(self) };
                let (command, stop) = (command.clone(), Arc::clone(&stop));
                offloaded.spawn(async move {
                    let status = Arc::clone(&shared).run_check(command.clone(), false).await;
                    if status != 0 && !shared.overflows(&command, status) {
                        stop.send_replace(true);
                    }
                    (index, status)
                });
            } else {
                local.push(command.clone());
            }
        }
        let mut failed = 0;
        if !local.is_empty() {
            let status = self.run_local(&local, &plan, &expensive, &stop).await;
            if status != 0 {
                stop.send_replace(true);
            }
            first_failure(&mut failed, status);
        }
        let mut results = Vec::new();
        while let Some(joined) = offloaded.join_next().await {
            results.push(joined.unwrap_or((usize::MAX, 1)));
        }
        results.sort_unstable();
        let mut overflowed = Vec::new();
        for (index, status) in results {
            match ordered.get(index) {
                Some(command) if self.overflows(command, status) => overflowed.push(command.clone()),
                _ => first_failure(&mut failed, status),
            }
        }
        Listed { failed, overflowed }
    }

    async fn scan(&self, skipped: &str) -> bool {
        let started = epoch();
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
        trace::record(&[self.span("publication scan", Category::Scans, started)]);
        passed
    }

    async fn git_push(&self, refspec: &str) -> bool {
        let started = epoch();
        let pushed = Command::new("git")
            .args(["push", "-q", "--no-verify", "origin", refspec])
            .current_dir(&self.root)
            .env("SAFE_PUSH_ACTIVE", "1")
            .stdin(Stdio::null())
            .status()
            .await
            .is_ok_and(|status| status.success());
        trace::record(&[self.span("git push of the published ref", Category::Git, started).with("refspec", refspec)]);
        pushed
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

    async fn start_actions(&self, commands: &[String]) -> Result<Option<(Child, PathBuf)>, i32> {
        if commands.is_empty() {
            return Ok(None);
        }
        if !self.scan("skipped: the lane cannot be published to GitHub Actions until the publication scan passes").await {
            return Err(1);
        }
        say(&format!("safe-push: running {} land check(s) on GitHub Actions ({})", commands.len(), self.workflow));
        let Ok((_, log)) = tempfile::Builder::new().tempfile().and_then(|file| file.keep().map_err(|error| error.error)) else { return Err(1) };
        let arguments = &self.arguments;
        let timeout = self.environment.timeout.unwrap_or(check_timings::CEILING);
        let spawned = Command::new(program())
            .args(["land-actions", "--workflow", &self.workflow, "--base", &arguments.base, "--head", &arguments.head, "--timeout", &timeout.to_string(), "--"])
            .args(commands)
            .current_dir(&self.root)
            .env("SAFE_PUSH_LAND_FAILED_LOG", &log)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn();
        match spawned {
            Ok(child) => Ok(Some((child, log))),
            Err(error) => {
                warn(&format!("safe-push: could not start north land-actions: {error}; NOT pushed."));
                let _ = fs::remove_file(&log);
                Err(1)
            }
        }
    }

    async fn finish_actions(&self, started: (Child, PathBuf), commands: &[String]) -> i32 {
        let (mut child, log) = started;
        let status = self.wait(&mut child).await;
        let mut failed = 0;
        if status == INFRASTRUCTURE {
            say(&format!("safe-push: GitHub Actions is unavailable (infrastructure, not a check failure); its {} land check(s) run on a build host", commands.len()));
            let fallback = Arc::new(self.without_actions(resolve_runner_host("build-host").await));
            failed = fallback.run_list(commands).await.status();
        } else if status != 0 {
            warn(&format!("safe-push: land checks on GitHub Actions exited {status}; NOT pushed."));
            self.outcome.record(&["event", "land checks on GitHub Actions", "failed", &format!("exit {status}")]);
            failed = status;
            if self.arguments.red_fix.is_empty() {
                self.red_main_check(&log).await;
            }
        }
        self.cache_record_as(status, commands, Some(ACTIONS_CLASS)).await;
        let _ = fs::remove_file(&log);
        failed
    }

    async fn on_actions(&self, commands: &[String]) -> i32 {
        match self.start_actions(commands).await {
            Ok(Some(started)) => self.finish_actions(started, commands).await,
            Ok(None) => 0,
            Err(status) => status,
        }
    }

    async fn execute_sharing_build(self: Arc<Self>, commands: Vec<String>) -> i32 {
        let phases = match shared_build::phases(commands) {
            Ok(phases) => phases,
            Err(commands) => return self.execute(commands).await,
        };
        say(&format!("safe-push: land check order: the north build the head-north checks share ({}), then {} head-north check(s), beside {} other land check(s)", phases.build, phases.head_north.len(), phases.others.len()));
        let others = async {
            if phases.others.is_empty() { 0 } else { Arc::clone(&self).execute(phases.others).await }
        };
        let shared = async {
            let built = Arc::clone(&self).execute(vec![phases.build]).await;
            if built != 0 {
                warn(&format!("safe-push: the north build the head-north checks share exited {built}, so the {} head-north check(s) did not run", phases.head_north.len()));
                return built;
            }
            Arc::clone(&self).execute(phases.head_north).await
        };
        let (others, shared) = tokio::join!(others, shared);
        if others != 0 { others } else { shared }
    }

    async fn execute(self: Arc<Self>, commands: Vec<String>) -> i32 {
        let arguments = &self.arguments;
        let placement = self.placement();
        let mut here = Vec::new();
        let mut on_actions = Vec::new();
        for command in commands {
            if placement.on_actions(self.portable(&command)) {
                on_actions.push(command);
            } else {
                here.push(command);
            }
        }
        if placement.runner && !on_actions.is_empty() {
            say(&format!("safe-push: no build-host slot is free, so {} land check(s) overflow to GitHub Actions", on_actions.len()));
        }
        let actions_started = epoch();
        let actions = match self.start_actions(&on_actions).await {
            Ok(started) => started,
            Err(status) => return status,
        };
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
        let listed = if here.is_empty() {
            Listed { failed: 0, overflowed: Vec::new() }
        } else {
            let shared = Arc::new(self.with_published(published.clone().unwrap_or_default()));
            shared.run_list(&here).await
        };
        let mut failed = listed.failed;
        if let Some(reference) = &published && !self.git_push(&format!(":refs/heads/{reference}")).await {
            warn(&format!("safe-push: could not delete origin {reference}"));
        }
        if let Some(started) = actions {
            first_failure(&mut failed, self.finish_actions(started, &on_actions).await);
            trace::record(&[self.span("checks on GitHub Actions", Category::Offload, actions_started).with("commands", on_actions.len().to_string())]);
        }
        first_failure(&mut failed, self.on_actions(&listed.overflowed).await);
        failed
    }
}

impl Shared {
    fn with_published(&self, published: String) -> Self {
        let mut shared = self.clone();
        shared.arguments.published = published;
        shared
    }

    fn without_actions(&self, host: String) -> Self {
        let mut shared = self.clone();
        shared.arguments.runner_host = host;
        shared.actions_down = true;
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

fn scope_for(root: &Path, arguments: &Arguments, commands: &[String]) -> Option<Scope> {
    if !commands.iter().any(|command| command.contains(PACKAGES_ENV) || command.contains(CHANGED_ENV)) {
        return None;
    }
    let scope = affected::scope(root, &arguments.base, &arguments.head).unwrap_or_else(|error| {
        warn(&format!("safe-push: {error}; the Rust checks cover the whole workspace"));
        Scope::Workspace
    });
    if !arguments.leased {
        say(&format!("safe-push: the Rust checks cover {}", scope.arguments()));
    }
    Some(scope)
}

fn defined_on_tree(root: &Path, arguments: &Arguments, commands: Vec<String>) -> Vec<String> {
    if !arguments.pinned() {
        return commands;
    }
    let tree = Config::load(root, arguments.tree());
    let tree = if arguments.after { tree.after_set() } else { tree };
    commands.into_iter().filter(|command| tree.runs(command)).collect()
}

fn it_for(root: &Path, arguments: &Arguments, commands: &[String]) -> Option<String> {
    if !commands.iter().any(|command| command.contains(IT_ENV)) {
        return None;
    }
    let selection = it_modules::selection(root, &arguments.base, &arguments.head).unwrap_or_else(|error| {
        warn(&format!("safe-push: {error}; the tests/it run covers every module"));
        it_modules::Selection::Every
    });
    if !arguments.leased {
        say(&format!("safe-push: the tests/it run is {}", selection.arguments()));
    }
    Some(selection.arguments())
}

fn portable(arguments: &Arguments) -> u8 {
    let Some(root) = toplevel() else { return 1 };
    let config = Config::load(&root, &arguments.head);
    let Ok(commands) = config.commands(&root, &arguments.base, &arguments.head) else { return 1 };
    let portable = config.workflow.is_empty() && !commands.is_empty() && commands.iter().all(|command| config.local_reason(command).is_none());
    u8::from(!portable)
}

fn record_wait(arguments: &Arguments) {
    let Some(log) = &arguments.event_log else { return };
    if let Some(directory) = log.parent() {
        let _ = fs::create_dir_all(directory);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(log) {
        let _ = writeln!(file, "{}", arguments.wait_event);
    }
}

fn start_stage(arguments: &Arguments) -> Option<u8> {
    let destination = arguments.destination.as_deref()?;
    if destination != "main" {
        return Some(0);
    }
    let Some(root) = toplevel() else { return Some(0) };
    if !Config::load(&root, "HEAD").has_checks() {
        return Some(0);
    }
    record_wait(arguments);
    let Err(error) = deps::sync(&root) else { return None };
    warn(&error);
    Outcome(arguments.outcome.clone()).record(&["skip", "skipped: bun install failed"]);
    Some(1)
}

fn any(arguments: &Arguments) -> u8 {
    let Some(root) = toplevel() else { return 1 };
    u8::from(!Config::load(&root, &arguments.head).has_checks())
}

fn replay(arguments: &Arguments) -> u8 {
    let until = if arguments.until == 0 { u64::MAX } else { arguments.until };
    let records: Vec<Record> = Records::at(arguments.records.clone().unwrap_or_else(Records::default_path)).read().into_iter().filter(|record| (arguments.since..until).contains(&record.started)).collect();
    say(&fail_fast::replay(&records).to_string());
    0
}

async fn orchestrate(arguments: Arguments) -> i32 {
    let Some(root) = toplevel() else {
        warn("safe-push: not in a git repo");
        return 1;
    };
    let config = Config::load(&root, &arguments.head);
    let local = !arguments.after && config.local_checks();
    let config = if arguments.after {
        config.after_set()
    } else if local {
        config.without_placement()
    } else {
        config
    };
    let (cancel_sender, cancel) = watch::channel(false);
    tokio::spawn(forward_signals(cancel_sender));
    let environment = if local { Environment::local() } else { Environment::load() };
    let commands = if arguments.leased {
        arguments.commands.clone()
    } else {
        match config.commands(&root, &arguments.base, &arguments.head) {
            Ok(commands) => defined_on_tree(&root, &arguments, commands),
            Err(message) => {
                warn(&format!("safe-push: {message}; NOT pushed."));
                return 1;
            }
        }
    };
    if !arguments.leased && !commands.is_empty() && commands.iter().all(|command| config.is_inferred(command)) {
        say(&format!("safe-push: no land.check lines in .safe-push; running the {} land check(s) inferred from the tree", commands.len()));
    }
    let scope = scope_for(&root, &arguments, &commands);
    let it = it_for(&root, &arguments, &commands);
    let trees = Trees::of(&root, &arguments.base, arguments.tree());
    let paths = path_class(&root, &arguments.base, &arguments.head);
    let selected_paths = arguments.pinned().then(|| changed(&root, &arguments.base, &arguments.head, &[]).join("\n"));
    let mut shared = Shared {
        selected_paths,
        trees,
        paths,
        fail_fast: false,
        workflow: config.workflow.clone(),
        outcome: Outcome(arguments.outcome.clone()),
        root,
        arguments,
        environment,
        config,
        cancel,
        free_slots: None,
        actions_down: false,
        scope,
        it,
    };
    if shared.arguments.leased {
        return Arc::new(shared).run_list(&commands).await.status();
    }
    let commands = shared.reuse(commands).await;
    if commands.is_empty() {
        return 0;
    }
    shared.arguments.runner_host = runner_host(&shared.environment, &shared.config).await;
    shared.free_slots = shared.count_free_slots().await;
    let shared = Arc::new(shared);
    let Some(plan) = clippy_first::plan(&commands, |command| !shared.config.is_inferred(command)).filter(|_| !shared.arguments.after) else {
        return shared.execute_sharing_build(commands).await;
    };
    say(&clippy_first::order(&plan, shared.scope.as_ref()));
    let status = Arc::clone(&shared).execute(plan.first()).await;
    if status != 0 {
        warn(&format!("safe-push: clippy on the affected crates exited {status}, so the other {} land check(s) did not run; NOT pushed.", plan.rest.len()));
        return status;
    }
    shared.execute(plan.rest).await
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
    if parsed.replay {
        return replay(&parsed);
    }
    if let Some(code) = start_stage(&parsed) {
        return code;
    }
    let mut parsed = parsed;
    let root = root_span(&parsed);
    parsed.span.clone_from(&root.id);
    let code = block_on(orchestrate(parsed)).unwrap_or_else(|error| {
        eprintln!("north land checks: {error}");
        1
    });
    trace::record(&[Span { end: epoch(), ..root }.with("exit", code.to_string())]);
    u8::try_from(code).unwrap_or(1)
}

fn root_span(arguments: &Arguments) -> Span {
    let parent = std::env::var(trace::PARENT_ENV).ok().filter(|parent| !parent.is_empty()).or_else(|| (!arguments.after).then(|| trace::check_id(&arguments.head)));
    let name = if arguments.after { "post-land checks" } else { "land checks" };
    let span = Span::new(&arguments.head, name, Category::Lander, epoch(), epoch()).on(&trace::this_host());
    let span = if arguments.after { span.with(trace::PHASE, trace::AFTER) } else { span };
    match parent {
        Some(parent) => span.under(&parent),
        None => span,
    }
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
    let scope = scope_for(&root, &arguments, &commands);
    let it = it_for(&root, &arguments, &commands);
    let trees = Trees::of(&root, &arguments.base, &arguments.head);
    let paths = path_class(&root, &arguments.base, &arguments.head);
    let shared = Arc::new(Shared { root, arguments, environment, config, outcome: Outcome(None), cancel, workflow: String::new(), trees, paths, fail_fast: false, free_slots: None, actions_down: false, scope, it , selected_paths: None });
    let pending = shared.cached(&commands).await;
    if pending.is_empty() { 0 } else { shared.run_list(&pending).await.status() }
}
