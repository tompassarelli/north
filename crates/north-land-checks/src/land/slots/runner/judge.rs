use std::fs;
use std::path::Path;
use std::time::SystemTime;

use north_base::foundation::exit::{ALREADY_PLACED_STATUS, NO_VERDICT_STATUS};

use crate::land::slots::select::Config;
use crate::landing::check_timings::outcomes::Outcome as CheckOutcome;
use crate::platform::land_queue::ROUND;
use super::{BASE_FAILS, BROKEN_BASE};
use crate::landing::check_findings;
use crate::landing::check_timings::{Base, BaseResult, Deadline, Timings};
use super::{Shared, host_name, say, timeout_row, warn};
use super::checkout::BaseTree;
use super::log::{log_findings, first_error};
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
            self.record_timeout(command, timeout, started);
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

    fn record_timeout(&self, command: &str, deadline: u64, started: u64) {
        let finished = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(started, |since| since.as_secs());
        let (base, candidate, tree) = (&self.arguments.base, &self.arguments.head, &self.trees.head);
        timeout_row::record(&self.environment.north, &self.root, &timeout_row::Timeout { base, candidate, tree, command, deadline, started, finished });
    }

    async fn judge_failure(&self, command: &str, leased: bool, timeout: u64, ran: &Ran) -> i32 {
        let (status, log) = (ran.status, ran.log.display());
        if status == NO_VERDICT_STATUS {
            let detail = format!("land check {command} reached no verdict (exit {NO_VERDICT_STATUS}: its host or connection failed before the check reported), which is not this lane's failure");
            warn(&format!("safe-push: {detail}; log: {log}; NOT pushed."));
            self.outcome.record(&["skip", &detail]);
            self.outcome.record(&["event", "land check", "no verdict", &format!("{command}; log: {log}")]);
            return NO_VERDICT_STATUS;
        }
        if let Some(judged) = self.judge_tests_against_base(command, leased, timeout, ran).await {
            return judged;
        }
        if let Some(first) = self.base_failure(command, leased, timeout).await {
            let tree = self.trees.base.get(..12).unwrap_or(&self.trees.base);
            if self.config.is_inferred(command) {
                let added = self.added_findings(command, &ran.log);
                if added.is_empty() {
                    say(&format!("safe-push: land check (inferred) {command} exited {status} on this lane {BASE_FAILS} {tree} ({first}), and the lane adds no finding the base lacks; the repository never declared it and main does not meet it, so it does not gate this lane; log: {log}"));
                    self.outcome.record(&["event", "land check", "not gating", &format!("{command}: main fails it too and the lane adds nothing; log: {log}")]);
                    return 0;
                }
                let shown = added.iter().take(5).cloned().collect::<Vec<_>>().join("; ");
                warn(&format!("safe-push: land check (inferred) {command} exited {status} on this lane {BASE_FAILS} {tree}, but the lane adds {} finding(s) the base lacks: {shown}; log: {log}; NOT pushed.", added.len()));
                self.cache_record(status, &[command.to_owned()]).await;
                self.outcome.record(&["event", "land check", "failed", &format!("{command}: adds {shown}; log: {log}")]);
                self.outcome.record(&["failed-step", command, &log.to_string()]);
                return status;
            }
            if let Some(host) = self.overloaded_host(command) {
                let detail = format!("land check {command} failed on this lane and on base tree {tree} ({first}) while this host ran at load {:.1} over its {} cores, so the load, not main or this lane, is suspect", host.load, host.cores);
                warn(&format!("safe-push: {detail}; log: {log}; NOT pushed."));
                self.outcome.record(&["skip", &detail]);
                self.outcome.record(&["event", "land check", "failed", &format!("{detail}; log: {log}")]);
                return NO_VERDICT_STATUS;
            }
            let detail = format!("{BROKEN_BASE}{command} on {}: {first}", host_name(&self.cache_class(command)));
            warn(&format!("safe-push: land check {command} exited {status} on this lane {BASE_FAILS} {tree}, so the failure is the check environment, not this lane; log: {log}; NOT pushed."));
            self.outcome.record(&["skip", &detail]);
            self.outcome.record(&["event", "land check", "failed", &format!("{detail}; log: {log}")]);
            return NO_VERDICT_STATUS;
        }
        warn(&format!("safe-push: land check {command} exited {status}; log: {log}; NOT pushed."));
        self.cache_record(status, &[command.to_owned()]).await;
        self.outcome.record(&["event", "land check", "failed", &format!("{command}; log: {log}")]);
        self.outcome.record(&["failed-step", command, &log.to_string()]);
        status
    }

    fn added_findings(&self, command: &str, lane_log: &Path) -> Vec<String> {
        let round = std::env::var(ROUND).unwrap_or_default();
        let timings = Timings::standard();
        let lane = log_findings(&fs::read_to_string(lane_log).unwrap_or_default());
        match timings.findings(&self.trees.base, command, &self.cache_class(command), &round) {
            Some(base) => check_findings::added(&lane, &check_findings::decode(&base)),
            None => lane.into_iter().chain(["the base tree's findings were not recorded".to_owned()]).collect(),
        }
    }

    async fn base_failure(&self, command: &str, leased: bool, timeout: u64) -> Option<String> {
        let round = std::env::var(ROUND).unwrap_or_default();
        let judged = !round.is_empty() && !*self.cancel.borrow() && !self.trees.base.is_empty() && self.trees.base != self.trees.head && Config::load(&self.root, &self.arguments.base).runs(command);
        if !judged {
            return None;
        }
        let class = self.cache_class(command);
        let timings = Timings::standard();
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

    async fn run_on_base(&self, command: &str, leased: bool, timeout: u64) -> BaseResult {
        let ran = match self.base_run(command, leased, timeout).await {
            Ok(ran) => ran,
            Err(error) => return BaseResult::Failed(error),
        };
        let _ = Timings::standard().record(&self.trees.base, command, &self.cache_class(command), ran.timing());
        if ran.status == 0 {
            let _ = fs::remove_file(&ran.log);
            return BaseResult::Passed;
        }
        let text = fs::read_to_string(&ran.log).unwrap_or_default();
        let _ = Timings::standard().record_findings(&self.trees.base, command, &self.cache_class(command), &std::env::var(ROUND).unwrap_or_default(), &check_findings::encode(&log_findings(&text)));
        let first = first_error(&text).unwrap_or_else(|| format!("exited {}", ran.status));
        say(&format!("safe-push: land check {command} exited {} on the base tree too; log: {}", ran.status, ran.log.display()));
        BaseResult::Failed(first)
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
