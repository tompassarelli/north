use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::{Child, Command};
use tokio::sync::watch;
use tokio::task::JoinSet;

use north_base::foundation::exit::NO_VERDICT_STATUS;

use crate::land::slots::shared_build;
use north_base::foundation::exit::SLOTS_BUSY_STATUS;
use crate::landing::check_timings::outcomes::Records;
use crate::platform::north_binary::land_north as program;
use crate::land::telemetry::epoch;
use crate::land::trace::{self, Category, Span};
use crate::landing::check_timings;
use super::fail_fast::Plan;
use super::{ACTIONS_CLASS, RED_MAIN_STATUS, Shared, batch, resolve_runner_host, say, steps, warn};
use super::spawn::{exit_code, under_capacity};

async fn forward_cancel(mut cancel: watch::Receiver<bool>, stop: Arc<watch::Sender<bool>>) {
    while cancel.changed().await.is_ok() {
        if *cancel.borrow() {
            stop.send_replace(true);
            return;
        }
    }
}

pub(super) fn first_failure(failed: &mut i32, status: i32) {
    if *failed == 0 || (matches!(*failed, NO_VERDICT_STATUS | SLOTS_BUSY_STATUS) && status != 0 && status != SLOTS_BUSY_STATUS) {
        *failed = status;
    }
}

pub(super) struct Listed {
    failed: i32,
    overflowed: Vec<String>,
}

impl Listed {
    pub(super) fn status(&self) -> i32 {
        if self.failed == 0 && !self.overflowed.is_empty() { SLOTS_BUSY_STATUS } else { self.failed }
    }
}

impl Shared {
    async fn run_local(self: &Arc<Self>, commands: &[String], plan: &Plan, expensive: &Arc<Self>, stop: &Arc<watch::Sender<bool>>) -> i32 {
        if self.environment.capacity.is_empty() || self.arguments.leased {
            let (leased, together) = (self.arguments.leased, side_by_side(commands.len()));
            let mut running = JoinSet::new();
            let mut statuses = Vec::new();
            for (index, command) in commands.iter().cloned().enumerate() {
                let (shared, stop) = (if plan.is_expensive(&command) { Arc::clone(expensive) } else { Arc::clone(self) }, Arc::clone(stop));
                running.spawn(async move {
                    let status = shared.run_check(command, leased).await;
                    if status != 0 {
                        stop.send_replace(true);
                    }
                    (index, status)
                });
                if !together {
                    statuses.extend(running.join_next().await.map(|joined| joined.unwrap_or((index, 1))));
                }
            }
            while let Some(joined) = running.join_next().await {
                statuses.push(joined.unwrap_or((usize::MAX, 1)));
            }
            statuses.sort_unstable();
            let mut failed = 0;
            for (_, status) in statuses {
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
        let mut child = match Command::new(&argv[0]).args(&argv[1..]).current_dir(&self.root).envs(self.scope_environment()).env_remove(north_base::foundation::offload::TREES_ENV).env(trace::PARENT_ENV, &lease.id).stdin(Stdio::null()).kill_on_drop(true).spawn() {
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

    fn spawn_batch(self: &Arc<Self>, ordered: &[String], plan: &Plan, stop: &Arc<watch::Sender<bool>>, offloaded: &mut JoinSet<Vec<(usize, i32)>>) -> Vec<usize> {
        let joined: Vec<usize> = ordered.iter().enumerate().filter(|(_, command)| self.batches(command, plan)).map(|(index, _)| index).collect();
        let Some(host) = joined.first().filter(|_| joined.len() >= 2).and_then(|first| self.offload_host_for(&ordered[*first])) else { return Vec::new() };
        let (shared, stop, indices) = (Arc::clone(self), Arc::clone(stop), joined.clone());
        let commands: Vec<String> = joined.iter().map(|index| ordered[*index].clone()).collect();
        offloaded.spawn(async move {
            let statuses = Arc::clone(&shared).run_batch(host, commands.clone()).await;
            if commands.iter().zip(&statuses).any(|(command, status)| *status != 0 && !shared.overflows(command, *status)) {
                stop.send_replace(true);
            }
            indices.into_iter().zip(statuses).collect::<Vec<_>>()
        });
        joined
    }

    fn batches(&self, command: &str, plan: &Plan) -> bool {
        self.offload_host_for(command).is_some() && !plan.is_expensive(command) && batch::joins(command) && self.config.host_inputs(command).is_empty()
    }

    async fn place_inputs(self: &Arc<Self>, ordered: &[String]) -> Arc<Self> {
        let needing: Vec<(&String, String, Vec<String>)> = ordered.iter().filter_map(|command| Some((command, self.offload_host_for(command)?, self.config.host_inputs(command)))).filter(|(_, _, inputs)| !inputs.is_empty()).collect();
        if needing.is_empty() || !self.environment.offload_host.is_empty() {
            return Arc::clone(self);
        }
        let here = north_base::platform::host::name();
        let mut placed = (*self.placed).clone();
        for (command, assigned, inputs) in needing {
            let mut choose = vec!["capacity", "offload", "choose", "--any", "--count", "1", "--prefer", assigned.as_str()];
            choose.extend(inputs.iter().flat_map(|input| ["--needs", input.as_str()]));
            let chosen = self.north(&choose).await.filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).filter(|host| !host.is_empty());
            let host = chosen.unwrap_or_else(|| here.clone());
            if host == here {
                say(&format!("safe-push: {command} runs on {here}, the lander host, since no build host that answers holds {}", inputs.join(" and ")));
            }
            placed.insert(command.clone(), host);
        }
        Arc::new(Self { placed: Arc::new(placed), ..(**self).clone() })
    }

    async fn spread(self: &Arc<Self>, ordered: &[String], plan: &Plan) -> Arc<Self> {
        let movable = |command: &String| self.offload_host_for(command).is_some() && !shared_build::uses_head_north(command);
        let joined: Vec<&String> = ordered.iter().filter(|command| self.batches(command, plan)).collect();
        let joined = if joined.len() >= 2 { joined } else { Vec::new() };
        let mut jobs: Vec<Vec<&String>> = Vec::new();
        let batched_here = !joined.is_empty() && !joined.iter().all(|command| movable(command));
        if !joined.is_empty() && !batched_here {
            jobs.push(joined.clone());
        }
        jobs.extend(ordered.iter().filter(|command| !joined.contains(command) && movable(command)).map(|command| vec![command]));
        let pinned = self.pinned + usize::from(batched_here) + ordered.iter().filter(|command| !joined.contains(command) && self.offload_host_for(command).is_some() && !movable(command)).count();
        if jobs.is_empty() || jobs.len() + pinned < 2 || !self.environment.offload_host.is_empty() {
            return Arc::clone(self);
        }
        let count = jobs.len().to_string();
        let mut choose = vec!["capacity", "offload", "choose", "--any", "--count", &count];
        for _ in 0..pinned {
            choose.extend(["--taken", self.arguments.runner_host.as_str()]);
        }
        let avoid = self.environment.off_host.then(north_base::platform::host::name);
        choose.extend(avoid.iter().flat_map(|here| ["--avoid", here.as_str()]));
        let Some(output) = self.north(&choose).await.filter(|output| output.status.success()) else { return Arc::clone(self) };
        let hosts: Vec<String> = String::from_utf8_lossy(&output.stdout).lines().map(str::trim).filter(|host| !host.is_empty()).map(str::to_owned).collect();
        if hosts.len() != jobs.len() {
            return Arc::clone(self);
        }
        let run_host = &self.arguments.runner_host;
        let elsewhere: Vec<&String> = hosts.iter().filter(|host| *host != run_host).collect();
        if !elsewhere.is_empty() {
            say(&format!("safe-push: {} of {} offloaded land check job(s) run on {} beside {pinned} head-north job(s) on {run_host}, by free slots", elsewhere.len(), jobs.len(), elsewhere.iter().map(|host| host.as_str()).collect::<Vec<_>>().join(", ")));
        }
        let placed = jobs.iter().zip(&hosts).flat_map(|(job, host)| job.iter().map(|command| ((*command).clone(), host.clone()))).collect();
        Arc::new(Self { placed: Arc::new(placed), ..(**self).clone() })
    }

    pub(super) async fn run_list(self: &Arc<Self>, commands: &[String]) -> Listed {
        let plan = steps::timed("plan", || Plan::of(commands, &self.paths, &Records::at(Records::default_path()).read()));
        if !plan.expensive.is_empty() {
            say(&format!("safe-push: {} expensive land check(s) run in fail-fast order and are cancelled at the first failure: {}", plan.expensive.len(), plan.expensive.join("; ")));
        }
        let ordered: Vec<String> = plan.ordered().cloned().collect();
        let this = steps::awaited("spread", async { self.spread(&ordered, &plan).await.place_inputs(&ordered).await }).await;
        let (stop, stopped) = watch::channel(false);
        let stop = Arc::new(stop);
        tokio::spawn(forward_cancel(this.cancel.clone(), Arc::clone(&stop)));
        let expensive = Arc::new(Self { cancel: stopped, fail_fast: true, ..(*this).clone() });
        let mut offloaded = JoinSet::new();
        let mut local = Vec::new();
        let joined = this.spawn_batch(&ordered, &plan, &stop, &mut offloaded);
        for (index, command) in ordered.iter().enumerate() {
            if joined.contains(&index) {
                continue;
            }
            if this.offload_host_for(command).is_some() {
                let shared = if plan.is_expensive(command) { Arc::clone(&expensive) } else { Arc::clone(&this) };
                let (command, stop) = (command.clone(), Arc::clone(&stop));
                offloaded.spawn(async move {
                    let status = Arc::clone(&shared).run_check(command.clone(), false).await;
                    if status != 0 && !shared.overflows(&command, status) {
                        stop.send_replace(true);
                    }
                    vec![(index, status)]
                });
            } else {
                local.push(command.clone());
            }
        }
        let mut failed = 0;
        if !local.is_empty() {
            let status = this.run_local(&local, &plan, &expensive, &stop).await;
            if status != 0 {
                stop.send_replace(true);
            }
            first_failure(&mut failed, status);
        }
        let mut results = Vec::new();
        while let Some(joined) = offloaded.join_next().await {
            results.extend(joined.unwrap_or_else(|_| vec![(usize::MAX, 1)]));
        }
        results.sort_unstable();
        let mut overflowed = Vec::new();
        for (index, status) in results {
            match ordered.get(index) {
                Some(command) if this.overflows(command, status) => overflowed.push(command.clone()),
                _ => first_failure(&mut failed, status),
            }
        }
        Listed { failed, overflowed }
    }

    async fn scan(&self, skipped: &str) -> bool {
        let started = epoch();
        let passed = Command::new(&self.arguments.safe_push)
            .arg0("safe-push")
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
            .args(["land", "red-check", "--main", &self.arguments.base, "--log"])
            .arg(log)
            .args(["--lane", &lane])
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .status()
            .await
            .map_or(1, exit_code);
        if status == RED_MAIN_STATUS {
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
            .args(["land", "actions", "--workflow", &self.workflow, "--base", &arguments.base, "--head", &arguments.head, "--timeout", &timeout.to_string(), "--"])
            .args(commands)
            .current_dir(&self.root)
            .env("SAFE_PUSH_LAND_FAILED_LOG", &log)
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn();
        match spawned {
            Ok(child) => Ok(Some((child, log))),
            Err(error) => {
                warn(&format!("safe-push: could not start north land actions: {error}; NOT pushed."));
                let _ = fs::remove_file(&log);
                Err(1)
            }
        }
    }

    async fn finish_actions(&self, started: (Child, PathBuf), commands: &[String]) -> i32 {
        let (mut child, log) = started;
        let status = self.wait(&mut child).await;
        let mut failed = 0;
        if status == NO_VERDICT_STATUS {
            say(&format!("safe-push: GitHub Actions is unavailable (infrastructure, not a check failure); its {} land check(s) run on a build host", commands.len()));
            let fallback = Arc::new(self.without_actions(resolve_runner_host(&program(), "build-host", &[]).await));
            failed = fallback.run_list(commands).await.status();
        } else if status != 0 {
            warn(&format!("safe-push: land checks on GitHub Actions exited {status}; NOT pushed."));
            self.outcome.record(&["event", "land checks on GitHub Actions", "failed", &format!("exit {status}")]);
            for command in commands {
                self.outcome.record(&["failed-step", command, ""]);
            }
            failed = status;
            if self.arguments.red_fix.is_empty() {
                self.red_main_check(&log).await;
            }
        }
        self.cache_record_as(status, commands, Some(ACTIONS_CLASS)).await;
        let _ = fs::remove_file(&log);
        failed
    }

    pub(super) async fn on_actions(&self, commands: &[String]) -> i32 {
        match self.start_actions(commands).await {
            Ok(Some(started)) => self.finish_actions(started, commands).await,
            Ok(None) => 0,
            Err(status) => status,
        }
    }

    pub(super) async fn execute_sharing_build(self: Arc<Self>, commands: Vec<String>) -> i32 {
        let phases = match shared_build::phases(commands) {
            Ok(phases) => phases,
            Err(commands) => return self.execute(commands).await,
        };
        if self.runs_on_one_build(&phases) && let Some(status) = self.run_on_one_build(&phases).await {
            return status;
        }
        say(&format!("safe-push: land check order: the north build the head-north checks share ({}), then {} head-north check(s), beside {} other land check(s)", phases.build, phases.head_north.len(), phases.others.len()));
        let pinned = if self.offload_host_for(&phases.build).is_some() { head_north_jobs(&phases.head_north).max(1) } else { 0 };
        let beside = Arc::new(Self { pinned, ..(*self).clone() });
        let others = async {
            if phases.others.is_empty() { 0 } else { beside.execute(phases.others).await }
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

    pub(super) async fn execute(self: Arc<Self>, commands: Vec<String>) -> i32 {
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

fn head_north_jobs(commands: &[String]) -> usize {
    let joined = commands.iter().filter(|command| batch::joins(command)).count();
    commands.len() - joined + if joined >= 2 { 1 } else { joined }
}

const SIDE_BY_SIDE_CORES: usize = 8;

fn side_by_side(checks: usize) -> bool {
    let configured = std::env::var("AGENT_CAPACITY_CORES").ok().and_then(|cores| cores.parse().ok()).filter(|cores: &usize| *cores > 0);
    checks > 1 && configured.unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from)) >= SIDE_BY_SIDE_CORES
}
