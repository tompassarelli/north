use std::collections::BTreeSet;
use std::fs;

use north_base::foundation::exit::NO_VERDICT_STATUS;

use crate::land::slots::select::{Config, changed};
use crate::land::slots::unit_reuse;
use crate::platform::land_queue::ROUND;
use super::attribution::{self, Attribution, LaneRun};
use super::spawn::Ran;
use super::{Shared, say, warn};


type Tests = BTreeSet<(String, String)>;

pub(super) fn failing_tests(log: &str) -> Tests {
    log.lines()
        .filter_map(|line| {
            let (took, rest) = line.trim().strip_prefix("FAIL [")?.split_once(']')?;
            let words: Vec<&str> = rest.split_whitespace().collect();
            let [.., binary, test] = words[..] else { return None };
            (took.trim().ends_with('s') && words.len() >= 3).then(|| (binary.to_owned(), test.to_owned()))
        })
        .collect()
}

pub(super) fn narrowed(command: &str, tests: &Tests) -> String {
    let filter = tests.iter().map(|(binary, test)| format!("(binary_id(={binary})&test(={test}))")).collect::<Vec<_>>().join("|");
    format!("export SAFE_PUSH_LAND_IT={}; {command}", unit_reuse::quote(&format!("--tests -E {filter}")))
}

fn names(tests: impl IntoIterator<Item = (String, String)>) -> String {
    tests.into_iter().map(|(binary, test)| format!("{binary} {test}")).collect::<Vec<_>>().join(", ")
}

impl Shared {
    /// Judges a nextest land check that failed before queueing the way the lander judges it on its base: reruns each failing test on the lane's base, and refuses the lane only for tests that pass there. `None` when the failure names no test or the base cannot be compared.
    pub(super) async fn judge_tests_against_base(&self, command: &str, leased: bool, timeout: u64, ran: &Ran) -> Option<i32> {
        let comparable = !self.arguments.after
            && !self.trees.base.is_empty()
            && self.trees.base != self.trees.head
            && std::env::var(ROUND).unwrap_or_default().is_empty()
            && !*self.cancel.borrow()
            && unit_reuse::runs_nextest(command)
            && !self.config.is_inferred(command)
            && Config::load(&self.root, &self.arguments.base).runs(command);
        if !comparable {
            return None;
        }
        let failing = failing_tests(&fs::read_to_string(&ran.log).unwrap_or_default());
        if failing.is_empty() {
            return None;
        }
        let tree = self.trees.base.get(..12).unwrap_or(&self.trees.base).to_owned();
        say(&format!("safe-push: {} test(s) failed on this lane; rerunning them on base tree {tree}", failing.len()));
        let base = self.base_run(&narrowed(command, &failing), leased, timeout).await.ok()?;
        if base.status == NO_VERDICT_STATUS {
            let _ = fs::remove_file(&base.log);
            return None;
        }
        let red_on_base = if base.status == 0 { Tests::new() } else { failing_tests(&fs::read_to_string(&base.log).unwrap_or_default()) };
        let _ = fs::remove_file(&base.log);
        let added: Tests = failing.difference(&red_on_base).cloned().collect();
        let log = ran.log.display();
        let lane_log = fs::read_to_string(&ran.log).unwrap_or_default();
        let cleared = added.is_empty() && self.base_takes_the_blame(command, &lane_log, &tree);
        if cleared {
            say(&format!("safe-push: land check {command} exited {}, and every failing test also fails on base tree {tree}: {}; the lane adds no failing test, so it is not refused", ran.status, names(failing.clone())));
            self.outcome.record(&["event", "land check", "not gating", &format!("{command}: {} failing test(s) are red on the base too; log: {log}", failing.len())]);
            let _ = fs::remove_file(&ran.log);
            return Some(0);
        }
        let shown = if added.is_empty() { format!("none, but the base cannot take the blame for {}", names(failing.clone())) } else { names(added.iter().cloned()) };
        let also = if red_on_base.is_empty() { String::new() } else { format!("; {} more fail on the base too and do not count", failing.len() - added.len()) };
        warn(&format!("safe-push: land check {command} exited {}: {} test(s) pass on base tree {tree} and fail on this lane: {shown}{also}; log: {log}; NOT pushed.", ran.status, added.len()));
        self.cache_record(ran.status, &[command.to_owned()]).await;
        self.outcome.record(&["event", "land check", "failed", &format!("{command}: fails {shown}; log: {log}")]);
        self.outcome.record(&["failed-step", command, &log.to_string()]);
        Some(ran.status)
    }

    /// Whether the base takes the blame for failing tests the base rerun already found red there: it judges only that the lane's run reached all its tests and compiled, failed in no environment, and changes none of the failing tests' files or packages, since the rerun decided which tests the lane adds.
    fn base_takes_the_blame(&self, command: &str, lane_log: &str, tree: &str) -> bool {
        let Ok(changed) = changed(&self.root, &self.arguments.base, &self.arguments.head, &[]) else { return false };
        let run = LaneRun { command, log: lane_log, changed: &changed, tests: true, reports_all: false };
        match attribution::attribute(&run, &attribution::signature(lane_log, true, &self.lane_tree()), &self.lane_tree()) {
            Attribution::Base => true,
            Attribution::Touches(files) => {
                say(&format!("safe-push: the failing tests also fail on base tree {tree}, but this lane changes their files ({}), so the failure counts as the lane's", files.join(", ")));
                false
            }
            Attribution::Unjudged(why) => {
                say(&format!("safe-push: the failing tests also fail on base tree {tree}, but {why}, so the failure counts as the lane's"));
                false
            }
            Attribution::Lane(_) => false,
        }
    }
}
