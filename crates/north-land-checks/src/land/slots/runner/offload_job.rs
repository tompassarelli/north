use std::fs;
use std::sync::Arc;
use std::time::SystemTime;

use crate::land::slots::shared_build::Phases;
use crate::land::slots::unit_reuse;
use north_base::verdict_cache::reuse_log;
use crate::land::trace::check_log::{self, Line};
use crate::land::trace::{self, Category, Span};
use super::execute::first_failure;
use super::log::new_log;
use super::spawn::{Attempt, Ran, shell_deadline};
use super::steps;
use super::{BATCH_GRACE, KILLED_STATUS, OFFLOAD_GRACE, Shared, TIMED_OUT_STATUS, batch, say, warn};

struct Job<'a> {
    host: &'a str,
    commands: &'a [String],
    timeouts: &'a [u64],
    script: String,
    outer: u64,
    what: &'a str,
    span: &'a str,
    dependents: usize,
}

impl Shared {
    pub(super) async fn run_batch(self: Arc<Self>, host: String, commands: Vec<String>) -> Vec<i32> {
        let timeouts: Vec<u64> = commands.iter().map(|command| self.deadline_of(command).seconds).collect();
        for (command, timeout) in commands.iter().zip(&timeouts) {
            self.outcome.record(&["ran"]);
            say(&format!("safe-push: land check: {command} ({timeout} s, in one {host} slot with {} other head-north check(s))", commands.len() - 1));
        }
        let checks: Vec<(&str, u64)> = commands.iter().map(String::as_str).zip(timeouts.iter().copied()).collect();
        let outer = timeouts.iter().copied().max().unwrap_or_default() + BATCH_GRACE;
        say(&format!("safe-push: offloading {} head-north land check(s) to {host} as one job", commands.len()));
        let job = Job { host: &host, commands: &commands, timeouts: &timeouts, script: batch::script(&checks), outer, what: "of the head-north batch", span: "head-north checks in one offload slot", dependents: 0 };
        let statuses = Arc::clone(&self).run_job(job).await;
        if self.overflows(&commands[0], statuses[0]) {
            say(&format!("safe-push: every build-host slot stayed busy, so {} head-north land check(s) overflow to GitHub Actions", commands.len()));
        }
        statuses
    }

    /// A lander probe's checks as one offload job on one north build, when it may run them all in a build-host slot; None when no slot came free, so they take the usual jobs.
    pub(super) async fn run_on_one_build(self: &Arc<Self>, phases: &Phases) -> Option<i32> {
        let host = self.offload_host_for(&phases.build)?;
        let commands: Vec<String> = std::iter::once(&phases.build).chain(&phases.head_north).chain(&phases.others).cloned().collect();
        let timeouts: Vec<u64> = commands.iter().map(|command| self.deadline_of(command).seconds).collect();
        for (command, timeout) in commands.iter().zip(&timeouts) {
            self.outcome.record(&["ran"]);
            say(&format!("safe-push: land check: {command} ({timeout} s, in one {host} slot on one north build with {} other land check(s))", commands.len() - 1));
        }
        let observed: Vec<String> = commands.iter().map(|command| self.observed(command)).collect();
        let checks: Vec<(&str, u64)> = observed.iter().map(String::as_str).zip(timeouts.iter().copied()).collect();
        let (head_north, others) = checks[1..].split_at(phases.head_north.len());
        let outer = (checks[0].1 + head_north.iter().map(|check| check.1).max().unwrap_or_default()).max(others.iter().map(|check| check.1).sum()) + BATCH_GRACE;
        say(&format!("safe-push: offloading the north build and {} land check(s) to {host} as one job", commands.len() - 1));
        let job = Job { host: &host, commands: &commands, timeouts: &timeouts, script: batch::on_one_build(checks[0], head_north, others), outer, what: "of the probe's one job", span: "a probe's checks on one build in one offload slot", dependents: phases.head_north.len() };
        let statuses = Arc::clone(self).run_job(job).await;
        if self.overflows(&commands[0], statuses[0]) {
            say("safe-push: every build-host slot stayed busy for the probe's one job, so its checks take their usual jobs");
            return None;
        }
        if statuses[0] != 0 && !phases.head_north.is_empty() {
            warn(&format!("safe-push: the north build the head-north checks share exited {}, so the {} head-north check(s) did not run", statuses[0], phases.head_north.len()));
        }
        Some(statuses.into_iter().fold(0, |mut failed, status| {
            first_failure(&mut failed, status);
            failed
        }))
    }

    pub(super) fn runs_on_one_build(&self, phases: &Phases) -> bool {
        let placement = self.placement();
        self.environment.lander_slot
            && self.environment.offload_host.is_empty()
            && !self.arguments.dry
            && std::iter::once(&phases.build).chain(&phases.head_north).chain(&phases.others).all(|command| self.offload_host_for(command).is_some() && self.config.host_inputs(command).is_empty() && !placement.on_actions(self.portable(command)))
    }

    async fn run_job(self: Arc<Self>, job: Job<'_>) -> Vec<i32> {
        let Job { host, commands, timeouts, script, outer, what, span, dependents } = job;
        let started = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |since| since.as_secs());
        let tree = self.arguments.tree().to_owned();
        let argv = shell_deadline(&self.environment.north, outer + OFFLOAD_GRACE, &self.offload_argv(host, &tree, outer, &script));
        let ran = match self.spawn_argv(what, argv, &tree, outer, &self.root).await {
            Ok(ran) => ran,
            Err(status) => return vec![status; commands.len()],
        };
        steps::waited(&ran.marks);
        if self.overflows(&commands[0], ran.status) {
            let _ = fs::remove_file(&ran.log);
            return vec![ran.status; commands.len()];
        }
        let line = Span::new(&self.arguments.head, span, Category::Offload, ran.started_at, ran.ended_at).under(&self.arguments.span).on(host).with("exit", ran.status.to_string());
        let mut spans = check_log::children(&Line { trace: &self.arguments.head, id: &line.id, category: Category::Offload, start: line.start, end: line.end, host }, &ran.marks);
        let running = spans.iter().map(|span| span.end).fold(ran.started_at, f64::max);
        spans.insert(0, line.clone());
        trace::record(&spans);
        let text = fs::read_to_string(&ran.log).unwrap_or_default();
        let mut statuses = Vec::new();
        for (index, ((command, timeout), part)) in commands.iter().zip(timeouts.iter().copied()).zip(batch::split(&text, commands.len())).enumerate() {
            if (1..=dependents).contains(&index) && statuses.first().is_some_and(|built| *built != 0) {
                statuses.push(statuses[0]);
                continue;
            }
            let Some(log) = new_log() else {
                statuses.push(1);
                continue;
            };
            let part = part.unwrap_or_else(|| batch::Part { status: if ran.status == 0 { 1 } else { ran.status }, offset: 0.0, seconds: ran.ended_at - running, text: text.clone() });
            let _ = fs::write(&log, &part.text);
            if self.observes(command) {
                reuse_log::harvest(&log, &unit_reuse::directory());
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let seconds = part.seconds.round() as u64;
            let started_at = running + part.offset;
            let checked = Ran { status: part.status, seconds, exceeded: part.status == TIMED_OUT_STATUS || (part.status == KILLED_STATUS && seconds >= timeout), log, started_at, ended_at: started_at + part.seconds, marks: Vec::new() };
            statuses.push(self.finish_check(command, Attempt { leased: false, started, timeout }, &checked, None, Some(&line.id)).await);
        }
        let _ = fs::remove_file(&ran.log);
        if self.environment.record_reuse && commands.iter().any(|command| self.observes(command)) {
            self.sync_store(host).await;
        }
        statuses
    }
}
