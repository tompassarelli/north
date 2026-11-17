use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use north_foundation::foundation::exit::{ALREADY_PLACED_STATUS, NO_VERDICT_STATUS, SLOTS_BUSY_STATUS};

use crate::land::slots::select::{Config, changed};
use crate::land::slots::unit_reuse;
use crate::landing::check_timings::outcomes::Outcome as CheckOutcome;
use north_base::platform::land_queue::ROUND;
use super::{BASE_FAILS, BROKEN_BASE};
use crate::landing::check_findings;
use crate::landing::check_timings::{Base, BaseResult, Deadline, Timings};
use super::{Shared, host_name, say, warn};
use super::checkout::BaseTree;
use super::lane_scope;
use super::attribution::{self, Attribution, LaneRun};
use super::log::first_error;
use super::spawn::{Attempt, Ended, Ran};

impl Shared {
    pub(super) fn deadline_of(&self, command: &str) -> Deadline {
        let timings = Timings::standard();
        self.environment.timeout.or_else(|| self.config.deadline_seconds(command)).map_or_else(|| timings.deadline(command, &self.cache_class(command)), Deadline::configured)
    }

    pub(super) async fn finish_check(&self, command: &str, attempt: Attempt, ran: &Ran, cpu: Option<f64>, parent: Option<&str>) -> i32 {
        let Attempt { leased, started, timeout } = attempt;
        let timings = Timings::standard();
        let class = self.cache_class(command);
        if self.overflows(command, ran.status) {
            say(&format!("safe-push: every build-host slot stayed busy, so land check {command} overflows to GitHub Actions"));
            let _ = fs::remove_file(&ran.log);
            return ran.status;
        }
        if ran.status == ALREADY_PLACED_STATUS {
            let _ = fs::remove_file(&ran.log);
            return ran.status;
        }
        self.trace_line(command, ran, cpu, parent);
        if self.cancelling() {
            let _ = fs::remove_file(&ran.log);
            return self.cancelled(command, started, ran.status, ran.seconds, cpu);
        }
        self.remember(command, started, CheckOutcome::of(ran.status), &Ended { status: ran.status, wall: ran.seconds, cpu, marks: &ran.marks });
        let _ = timings.record(&self.trees.head, command, &class, ran.timing());
        if ran.exceeded {
            let base = timings.base(&self.trees.base, command, &class, timeout);
            if base == Base::Exceeded {
                let _ = timings.exceeded_on_base_too(command, &class, timeout);
            }
            return self.judge_hang(command, timeout, base, &ran.log);
        }
        let commands = [command.to_owned()];
        if ran.status != 0 {
            return self.judge_failure(command, leased, timeout, ran).await;
        }
        let _ = timings.passed(command, &class, ran.seconds);
        self.cache_record(0, &commands).await;
        let _ = fs::remove_file(&ran.log);
        0
    }

    async fn judge_failure(&self, command: &str, leased: bool, timeout: u64, ran: &Ran) -> i32 {
        let (status, log) = (ran.status, ran.log.display());
        if matches!(status, NO_VERDICT_STATUS | SLOTS_BUSY_STATUS) {
            let detail = format!("land check {command} reached no verdict (exit {status}: its host, its connection or the offload slot line ended it before the check reported), which is not this lane's failure");
            warn(&format!("safe-push: {detail}; log: {log}; NOT pushed."));
            self.outcome.record(&["skip", &detail]);
            self.outcome.record(&["event", "land check", "no verdict", &format!("{command}; log: {log}")]);
            return status;
        }
        if let Some(judged) = self.judge_tests_against_base(command, leased, timeout, ran).await {
            return judged;
        }
        if let Some(judged) = self.judge_against_base(command, leased, timeout, ran).await {
            return judged;
        }
        warn(&format!("safe-push: land check {command} exited {status}; log: {log}; NOT pushed."));
        self.cache_record(status, &[command.to_owned()]).await;
        self.outcome.record(&["event", "land check", "failed", &format!("{command}; log: {log}")]);
        if self.lane_scoped(command) {
            let lanes = lane_scope::failed(&fs::read_to_string(&ran.log).unwrap_or_default()).join(",");
            self.outcome.record(&["failed-step", command, &log.to_string(), &lanes]);
        } else {
            self.outcome.record(&["failed-step", command, &log.to_string()]);
        }
        status
    }

    async fn judge_against_base(&self, command: &str, leased: bool, timeout: u64, ran: &Ran) -> Option<i32> {
        let (first, base) = self.base_findings(command, leased, timeout).await?;
        let (status, log) = (ran.status, ran.log.display());
        let tree = self.trees.base.get(..12).unwrap_or(&self.trees.base);
        let held = !std::env::var(ROUND).unwrap_or_default().is_empty() && !self.config.is_inferred(command);
        if held && let Some(host) = self.overloaded_host(command) {
            let detail = format!("land check {command} failed on this lane and on base tree {tree} ({first}) while this host ran at load {:.1} over its {} cores, so the load, not main or this lane, is suspect", host.load, host.cores);
            warn(&format!("safe-push: {detail}; log: {log}; NOT pushed."));
            self.outcome.record(&["skip", &detail]);
            self.outcome.record(&["event", "land check", "failed", &format!("{detail}; log: {log}")]);
            return Some(NO_VERDICT_STATUS);
        }
        let text = fs::read_to_string(&ran.log).unwrap_or_default();
        let Ok(changed) = changed(&self.root, &self.arguments.base, &self.arguments.head, &[]) else {
            say(&format!("safe-push: land check {command} {BASE_FAILS} {tree}, but this lane's changed files could not be listed, so the failure counts as the lane's"));
            return None;
        };
        let run = LaneRun { command, log: &text, changed: &changed, tests: unit_reuse::runs_tests(command), reports_all: self.config.is_inferred(command) || Config::load(&self.root, &self.arguments.base).reports_all(command) };
        let attributed = if base.is_empty() { Attribution::Unjudged("the base tree's failure names nothing to compare") } else { attribution::attribute(&run, &base, &self.lane_tree()) };
        match attributed {
            Attribution::Base => {
                say(&format!("safe-push: land check {command} fails on this lane (exit {status}) the same way it fails on base tree {tree}, and the lane changes none of its own files, so main's failure stays red on main and does not refuse this lane; log: {log}"));
                self.outcome.record(&["event", "land check", "not gating", &format!("{command}: fails the same way on base tree {tree}; log: {log}")]);
                let _ = fs::remove_file(&ran.log);
                Some(0)
            }
            Attribution::Lane(added) => {
                let shown = added.iter().take(5).cloned().collect::<Vec<_>>().join("; ");
                say(&format!("safe-push: land check {command} {BASE_FAILS} {tree}, but this lane adds {} failure(s) the base lacks: {shown}", added.len()));
                None
            }
            Attribution::Touches(files) if self.arguments.queueing => {
                say(&format!("safe-push: land check {command} {BASE_FAILS} {tree}, and this lane changes its own files ({}); the run before queueing accepts it, since main fails it too; log: {log}", files.join(", ")));
                self.outcome.record(&["event", "land check", "not gating", &format!("{command}: fails the same way on base tree {tree} and changes its own files; log: {log}")]);
                let _ = fs::remove_file(&ran.log);
                Some(0)
            }
            Attribution::Touches(files) => {
                say(&format!("safe-push: land check {command} {BASE_FAILS} {tree}, but this lane changes its own files ({}), so the failure counts as the lane's", files.join(", ")));
                None
            }
            Attribution::Unjudged(why) if held => {
                let detail = format!("{BROKEN_BASE}{command} on {}: {first}", host_name(&self.cache_class(command)));
                warn(&format!("safe-push: land check {command} exited {status} on this lane {BASE_FAILS} {tree}, and {why}; log: {log}; NOT pushed."));
                self.outcome.record(&["skip", &detail]);
                self.outcome.record(&["event", "land check", "failed", &format!("{detail}; log: {log}")]);
                Some(NO_VERDICT_STATUS)
            }
            Attribution::Unjudged(why) => {
                say(&format!("safe-push: land check {command} {BASE_FAILS} {tree}, but {why}, so the failure counts as the lane's"));
                None
            }
        }
    }

    /// The base tree's failure of `command`, `None` when the base passes it or cannot judge it. A round reuses the base result
    /// it recorded in the lander's own state under its round key; a run before queueing reads no record, since a caller's state
    /// home is its own to write, and runs the check once at the base instead.
    async fn base_findings(&self, command: &str, leased: bool, timeout: u64) -> Option<(String, BTreeSet<String>)> {
        let round = std::env::var(ROUND).unwrap_or_default();
        let judged = !self.arguments.after && !self.lane_scoped(command) && !*self.cancel.borrow() && !self.trees.base.is_empty() && self.trees.base != self.trees.head && Config::load(&self.root, &self.arguments.base).runs(command);
        if !judged {
            return None;
        }
        let tree = self.trees.base.get(..12).unwrap_or(&self.trees.base);
        if !round.is_empty() {
            let (timings, class) = (Timings::standard(), self.cache_class(command));
            match timings.result(&self.trees.base, command, &class, &round) {
                Some(BaseResult::Passed) => {
                    say(&format!("safe-push: base tree {tree} already ran land check {command} this round and passed it"));
                    return None;
                }
                Some(BaseResult::Failed(first)) => {
                    say(&format!("safe-push: base tree {tree} already ran land check {command} this round and failed it"));
                    return Some((first, timings.findings(&self.trees.base, command, &class, &round).map(|found| check_findings::decode(&found)).unwrap_or_default()));
                }
                None => {}
            }
        }
        say(&format!("safe-push: land check failed; running it once on base tree {tree} to tell whose failure it is: {command}"));
        self.run_on_base(command, leased, timeout, Some(round.as_str()).filter(|round| !round.is_empty())).await
    }

    pub(super) async fn base_run(&self, command: &str, leased: bool, timeout: u64) -> Result<Ran, String> {
        let base = self.arguments.base.clone();
        let tree = if self.offload_host_for(command).is_some() {
            None
        } else {
            Some(BaseTree::detached(&self.root, &base)?)
        };
        let dir = tree.as_ref().map_or(self.root.as_path(), BaseTree::path);
        let ran = self.spawn_check(command, leased, &base, timeout, dir).await;
        ran.map_err(|_| format!("land check {command} could not start on the base tree"))
    }

    async fn run_on_base(&self, command: &str, leased: bool, timeout: u64, round: Option<&str>) -> Option<(String, BTreeSet<String>)> {
        let ran = match self.base_run(command, leased, timeout).await {
            Ok(ran) => ran,
            Err(error) => return Some((error, BTreeSet::new())),
        };
        let (timings, class) = (Timings::standard(), self.cache_class(command));
        let _ = timings.record(&self.trees.base, command, &class, ran.timing());
        if ran.status == 0 {
            let _ = fs::remove_file(&ran.log);
            if let Some(round) = round {
                let _ = timings.record_result(&self.trees.base, command, &class, round, &BaseResult::Passed);
            }
            return None;
        }
        let text = fs::read_to_string(&ran.log).unwrap_or_default();
        let found = attribution::signature(&text, unit_reuse::runs_tests(command), &self.lane_tree());
        let first = first_error(&text).unwrap_or_else(|| format!("exited {}", ran.status));
        if let Some(round) = round {
            let _ = timings.record_findings(&self.trees.base, command, &class, round, &check_findings::encode(&found));
            let _ = timings.record_result(&self.trees.base, command, &class, round, &BaseResult::Failed(first.clone()));
        }
        say(&format!("safe-push: land check {command} exited {} on the base tree too; log: {}", ran.status, ran.log.display()));
        Some((first, found))
    }

    fn judge_hang(&self, command: &str, timeout: u64, base: Base, log: &Path) -> i32 {
        let tree = self.trees.base.get(..12).unwrap_or(&self.trees.base);
        let base = match base {
            Base::Within(seconds) => format!("finished it in {seconds} s on an earlier run; a wall-clock deadline moves with the host's load and the probes beside it, so the hang is retried, and a lane that hangs on every try is refused as a persistent transient"),
            Base::Exceeded => "also ran past it, so the hang is infrastructure, not this lane".to_owned(),
            Base::Untimed => "has never been timed, so the hang is infrastructure, not this lane".to_owned(),
        };
        let detail = format!("land check {command} ran past its {timeout} s deadline and base tree {tree} {base}");
        warn(&format!("safe-push: {detail}; log: {}; NOT pushed.", log.display()));
        self.outcome.record(&["skip", &detail]);
        self.outcome.record(&["event", "land check", "failed", &format!("{detail}; log: {}", log.display())]);
        NO_VERDICT_STATUS
    }

    fn overloaded_host(&self, command: &str) -> Option<HostLoad> {
        if self.offload_host_for(command).is_some_and(|host| host != north_base::platform::host::name()) {
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
