use std::fs;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Instant, SystemTime};
use north_base::foundation::carry;
use rustix::process::{Pid, Signal, kill_process};
use tokio::process::{Child, Command};

use crate::land::slots::select::Placement;
use crate::land::slots::{unit_reuse, verdict_key};
use crate::landing::check_timings::outcomes::{Outcome as CheckOutcome, Record, Records};
use crate::platform::north_binary::land_north as program;
use crate::land::telemetry::epoch;
use crate::land::trace::{self, Category, Span};
use crate::land::trace::check_log::{self, Line, Mark};
use crate::landing::check_timings::Timing;
use super::{ACTIONS_CLASS, CANCELLED_STATUS, KILLED_STATUS, OFFLOAD_GRACE, REAP, Shared, TIMED_OUT_STATUS, say, warn};
use super::log::{new_log, Sink, tee};

const STORE_SYNC: std::time::Duration = std::time::Duration::from_secs(90);

#[derive(Clone, Copy)]
pub(super) struct Attempt {
    pub(super) leased: bool,
    pub(super) started: u64,
    pub(super) timeout: u64,
}

pub(super) struct Ran {
    pub(super) status: i32,
    pub(super) seconds: u64,
    pub(super) exceeded: bool,
    pub(super) log: PathBuf,
    pub(super) started_at: f64,
    pub(super) ended_at: f64,
    pub(super) marks: Vec<Mark>,
}

impl Ran {
    pub(super) fn timing(&self) -> Timing {
        if self.exceeded { Timing::Exceeded(self.seconds) } else { Timing::Finished(self.seconds) }
    }
}

pub(super) fn exit_code(status: ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status.code().or_else(|| status.signal().map(|signal| 128 + signal)).unwrap_or(1)
}

pub(super) fn shell_deadline(seconds: u64, argv: &[String]) -> Vec<String> {
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

pub(super) fn under_capacity(capacity: &str, argv: Vec<String>) -> Vec<String> {
    if capacity.is_empty() {
        return argv;
    }
    let mut command = vec!["bash".to_owned(), "-euo".to_owned(), "pipefail".to_owned(), "-c".to_owned(), format!("exec {capacity} \"$@\""), "safe-push-capacity".to_owned()];
    command.extend(argv);
    command
}

impl Shared {
    pub(super) async fn wait(&self, child: &mut Child) -> i32 {
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

    pub(super) fn offload_host_for(&self, command: &str) -> Option<String> {
        if !self.environment.offload_host.is_empty() {
            return Some(self.environment.offload_host.clone());
        }
        (!self.arguments.runner_host.is_empty() && self.config.local_reason(command).is_none()).then(|| self.placed.get(command).unwrap_or(&self.arguments.runner_host).clone())
    }

    pub(super) fn scope_environment(&self) -> Vec<(&'static str, String)> {
        verdict_key::selection_environment(self.scope.as_ref(), self.it.as_deref(), self.selected_paths.as_deref())
    }

    fn land_runner(&self) -> String {
        self.environment.runner.clone().unwrap_or_else(|| self.config.runner.clone().unwrap_or_default())
    }

    pub(super) fn placement(&self) -> Placement {
        Placement { workflow: !self.workflow.is_empty() && !self.actions_down, runner: !self.land_runner().is_empty(), reserve: !self.environment.offload_host.is_empty(), free_slots: self.free_slots }
    }

    pub(super) fn portable(&self, command: &str) -> bool {
        self.config.off_actions_reason(command).is_none()
    }

    pub(super) fn overflows(&self, command: &str, status: i32) -> bool {
        self.placement().overflows(status, self.offload_host_for(command).is_some(), self.portable(command))
    }

    pub(super) async fn count_free_slots(&self) -> Option<u32> {
        let host = &self.arguments.runner_host;
        if !self.placement().probes_slots() || host.is_empty() {
            return None;
        }
        let Some(output) = self.north(&["capacity", "offload", "free", "--host", host]).await else { return Some(0) };
        let free = if output.status.success() { String::from_utf8_lossy(&output.stdout).trim().parse().unwrap_or(0) } else { 0 };
        say(&format!("safe-push: {host} has {free} free land-check slot(s)"));
        Some(free)
    }

    pub(super) fn cache_class(&self, command: &str) -> String {
        if !self.environment.offload_host.is_empty() {
            return self.environment.offload_host.clone();
        }
        if self.placement().on_actions(self.portable(command)) {
            return ACTIONS_CLASS.to_owned();
        }
        self.offload_host_for(command).unwrap_or_else(|| "local".to_owned())
    }

    pub(super) async fn north(&self, arguments: &[&str]) -> Option<std::process::Output> {
        Command::new(program()).args(arguments).current_dir(&self.root).stdin(Stdio::null()).output().await.ok()
    }

    pub(super) fn offload_argv(&self, host: &str, tree: &str, timeout: u64, command: &str) -> Vec<String> {
        vec![program(), "capacity".into(), "offload".into(), "run".into(), "--host".into(), host.to_owned(), "--base".into(), self.arguments.base.clone(), "--head".into(), tree.to_owned(), "--timeout".into(), timeout.to_string(), "--".into(), command.to_owned()]
    }

    pub(super) fn observes(&self, command: &str) -> bool {
        !self.arguments.after && unit_reuse::runs_nextest(command)
    }

    pub(super) fn observed(&self, command: &str) -> String {
        if self.observes(command) { unit_reuse::wrapped("north", &self.arguments.head, self.environment.record_reuse, command) } else { command.to_owned() }
    }

    pub(super) async fn sync_store(&self, host: &str) {
        let arguments = ["capacity", "offload", "verdict", "sync", "--host", host];
        if tokio::time::timeout(STORE_SYNC, self.north(&arguments)).await.is_err() {
            warn(&format!("safe-push: syncing the verdict store with {host} took over {} s; the run goes on without it", STORE_SYNC.as_secs()));
        }
    }

    pub(super) async fn spawn_check(&self, command: &str, leased: bool, tree: &str, timeout: u64, dir: &Path) -> Result<Ran, i32> {
        let environment = &self.environment;
        let observing = self.observes(command);
        let argv = if let Some(host) = self.offload_host_for(command) {
            if !environment.offload_host.is_empty() && self.config.local_reason(command).is_some() {
                warn(&format!("safe-push: land check {command} needs this host, which is in game-reserve; NOT pushed."));
                return Err(1);
            }
            let observed = self.observed(command);
            if observing {
                self.sync_store(&host).await;
            }
            let offload = self.offload_argv(&host, tree, timeout, &observed);
            let capacity = if environment.offload_host.is_empty() { "" } else { environment.capacity.as_str() };
            let suffix = if environment.offload_host.is_empty() { "" } else { " while this host is in game-reserve" };
            say(&format!("safe-push: offloading to {host}{suffix}"));
            under_capacity(capacity, shell_deadline(timeout + OFFLOAD_GRACE, &offload))
        } else {
            let observed = if observing { unit_reuse::wrapped(&unit_reuse::quote(&program()), &self.arguments.head, environment.record_reuse, command) } else { command.to_owned() };
            let local = ["bash".to_owned(), "-euo".to_owned(), "pipefail".to_owned(), "-c".to_owned(), observed];
            let capacity = if leased { "" } else { environment.capacity.as_str() };
            under_capacity(capacity, shell_deadline(timeout, &local))
        };
        let ran = self.spawn_argv(command, argv, tree, timeout, dir).await;
        if observing && let Ok(ran) = &ran {
            unit_reuse::harvest::harvest(&ran.log, &unit_reuse::harvest::directory());
            if environment.record_reuse && let Some(host) = self.offload_host_for(command) {
                self.sync_store(&host).await;
            }
        }
        ran
    }

    pub(super) async fn spawn_argv(&self, command: &str, argv: Vec<String>, tree: &str, timeout: u64, dir: &Path) -> Result<Ran, i32> {
        let Some(log) = new_log() else {
            warn("safe-push: could not create a land check log; NOT pushed.");
            return Err(1);
        };
        let started = Instant::now();
        let started_at = epoch();
        let mut child = match Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(dir)
            .envs(self.scope_environment())
            .env("SAFE_PUSH_LAND_BASE", &self.arguments.base)
            .env("SAFE_PUSH_LAND_HEAD", tree)
            .env("SAFE_PUSH_LAND_RUNTIME_HEAD", self.arguments.tree())
            .env("SAFE_PUSH_LAND_REF", &self.arguments.published)
            .env("SAFE_PUSH_LANDING", "1")
            .env(carry::LAND_HOST_INPUTS, self.config.host_inputs(command).join(" "))
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
        let exceeded = status == TIMED_OUT_STATUS || (status == KILLED_STATUS && seconds >= timeout);
        Ok(Ran { status, seconds, exceeded, log, started_at, ended_at, marks })
    }

    pub(super) fn span(&self, name: &str, category: Category, start: f64) -> Span {
        Span::new(&self.arguments.head, name, category, start, epoch()).under(&self.arguments.span).on(&trace::this_host())
    }

    pub(super) fn trace_line(&self, command: &str, ran: &Ran, cpu: Option<f64>, parent: Option<&str>) {
        let category = Category::of_command(command);
        let host = self.offload_host_for(command).unwrap_or_else(trace::this_host);
        let head = &self.arguments.head;
        let line = Span::new(head, command, category, ran.started_at, ran.ended_at).under(parent.unwrap_or(&self.arguments.span)).on(&host).cpu(cpu).with("cache", "miss").with("exit", ran.status.to_string());
        let mut spans = check_log::children(&Line { trace: head, id: &line.id, category, start: line.start, end: line.end, host: &host }, &ran.marks);
        spans.insert(0, line);
        trace::record(&spans);
    }

    pub(super) fn remember(&self, command: &str, started: u64, status: i32, outcome: CheckOutcome, wall: u64, cpu: Option<f64>) {
        #[allow(clippy::cast_precision_loss)]
        let wall = wall as f64;
        let record = Record { started, landing: self.arguments.head.clone(), paths: self.paths.clone(), outcome, exit: status, wall, cpu, command: command.to_owned() };
        let _ = Records::at(Records::default_path()).append(&record);
    }

    pub(super) fn cancelled(&self, command: &str, started: u64, status: i32, wall: u64, cpu: Option<f64>) -> i32 {
        say(&format!("safe-push: land check {command} cancelled: an earlier land check failed"));
        self.remember(command, started, status, CheckOutcome::Cancelled, wall, cpu);
        self.outcome.record(&["event", "land check", "cancelled", &format!("{command}: an earlier land check failed")]);
        0
    }

    pub(super) async fn run_check(self: Arc<Self>, command: String, leased: bool) -> i32 {
        let started = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs());
        if self.fail_fast && *self.cancel.borrow() {
            return self.cancelled(&command, started, CANCELLED_STATUS, 0, None);
        }
        self.outcome.record(&["ran"]);
        if !self.workflow.is_empty() && self.offload_host_for(&command).is_none() && let Some(reason) = self.config.off_actions_reason(&command) {
            say(&format!("safe-push: land check runs on this host because it {reason}"));
        }
        let deadline = self.deadline_of(&command);
        let timeout = deadline.seconds;
        let label = if self.config.is_inferred(&command) { " (inferred)" } else { "" };
        say(&format!("safe-push: land check{label}: {command} ({deadline}; started {})", north_base::foundation::clock::Timestamp::now().iso_millis()));
        let measured = self.offload_host_for(&command).is_none();
        let before = children_cpu_seconds();
        let ran = match self.spawn_check(&command, leased, self.arguments.tree(), timeout, &self.root).await {
            Ok(ran) => ran,
            Err(status) => return status,
        };
        let cpu = before.zip(children_cpu_seconds()).filter(|_| measured).map(|(before, after)| after - before);
        self.finish_check(&command, Attempt { leased, started, timeout }, &ran, cpu, None).await
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
