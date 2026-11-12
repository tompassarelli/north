use std::collections::BTreeSet;
use std::fs;
use std::process::{Command, Stdio};

use north_base::foundation::offload::OWNER_ENV;

use super::{Landed, Run, check, failing, superseded};
use crate::land::after::follow;
use crate::land::blame;
use crate::land::candidate::git;
use crate::land::checks::Verdict;
use crate::landing::red_check::{self, Failure};

/// What a prefix probe runs on a tree: the failed tests alone, else the failed check lines alone, else the whole
/// after-landing check when the red run named neither.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Probe {
    Tests(BTreeSet<Failure>),
    Steps(Vec<String>),
    Checks,
}

impl Probe {
    pub(crate) fn of(failing: &BTreeSet<Failure>) -> Self {
        let tests: BTreeSet<Failure> = failing.iter().filter(|failure| matches!(failure, Failure::Cargo(_) | Failure::Harness(_))).cloned().collect();
        if !tests.is_empty() {
            return Self::Tests(tests);
        }
        let steps: Vec<String> = failing
            .iter()
            .filter_map(|failure| match failure {
                Failure::Step(command) => Some(command.clone()),
                _ => None,
            })
            .collect();
        if steps.is_empty() { Self::Checks } else { Self::Steps(steps) }
    }

    pub(crate) fn runs(&self, failure: &Failure) -> bool {
        match (self, failure) {
            (Self::Tests(tests), failure) => tests.contains(failure),
            (Self::Steps(steps), Failure::Step(command)) => steps.contains(command),
            (Self::Steps(_), _) => false,
            (Self::Checks, _) => true,
        }
    }
}

/// Runs `probe` on `tree`, logging to `run.log`, and reports the failing checks it reached.
pub(super) fn probe(landed: &Landed, run: &Run, tree: &str, probe: &Probe) -> blame::Probe {
    match probe {
        Probe::Checks => checked(landed, tree, run),
        Probe::Steps(steps) => {
            let mut runner = vec![follow::program(), "land".to_owned(), "checks".to_owned(), "--after".to_owned(), "--fresh".to_owned(), "--dry".to_owned(), "1".to_owned()];
            runner.extend(steps.iter().flat_map(|step| ["--command".to_owned(), step.clone()]));
            checked(landed, tree, &Run { runner, ..run.clone() })
        }
        Probe::Tests(tests) => tested(landed, run, tree, tests),
    }
}

fn checked(landed: &Landed, tree: &str, run: &Run) -> blame::Probe {
    let result = check(landed, tree, run);
    let reached = match &result.verdict {
        Verdict::RedMain(_) => failing(&run.log, &run.outcome).into_iter().map(|failure| failure.label().to_owned()).collect(),
        _ => BTreeSet::new(),
    };
    blame::Probe { verdict: result.verdict, failing: reached }
}

/// Runs the failed tests that exist at `tree` there; a harness test absent at the tree counts as a pass, and so does a
/// Cargo test, since an exact filter naming no test runs none.
fn tested(landed: &Landed, run: &Run, tree: &str, tests: &BTreeSet<Failure>) -> blame::Probe {
    if run.superseded() {
        return blame::Probe { verdict: superseded(landed), failing: BTreeSet::new() };
    }
    let present: BTreeSet<Failure> = tests
        .iter()
        .filter(|test| match test {
            Failure::Harness(path) => git(&landed.repo, &["cat-file", "-e", &format!("{tree}:{path}")]).is_ok(),
            _ => true,
        })
        .cloned()
        .collect();
    let Some(command) = red_check::rerun_command(&present) else { return blame::Probe { verdict: Verdict::Green, failing: BTreeSet::new() } };
    let host = run.host.clone().unwrap_or_else(north_base::platform::host::name);
    let timeout = run.deadline.as_secs().to_string();
    let ran = Command::new(follow::program())
        .args(["capacity", "offload", "run", "--host", &host, "--base", tree, "--head", tree, "--timeout", &timeout, "--", &command])
        .current_dir(&landed.repo)
        .env(OWNER_ENV, &run.offload_owner)
        .stdin(Stdio::null())
        .output();
    let output = match ran {
        Ok(output) => output,
        Err(error) => return blame::Probe { verdict: Verdict::Transient(format!("could not run the failed tests on {tree} on {host}: {error}")), failing: BTreeSet::new() },
    };
    let log = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    if let Some(parent) = run.log.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&run.log, &log);
    let reached: BTreeSet<String> = red_check::failures(&log).into_iter().filter(|failure| present.contains(failure)).map(|failure| failure.label().to_owned()).collect();
    let verdict = if output.status.success() {
        Verdict::Green
    } else if reached.is_empty() {
        Verdict::Transient(format!("the failed tests exited non-zero without failing one of them, so they reached no verdict; log: {}", run.log.display()))
    } else {
        Verdict::RedMain(format!("{} failed; log: {}", reached.iter().cloned().collect::<Vec<_>>().join(", "), run.log.display()))
    };
    blame::Probe { verdict, failing: if output.status.success() { BTreeSet::new() } else { reached } }
}
