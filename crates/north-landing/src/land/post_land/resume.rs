use super::{Cancel, Landed, Outcome, Search, after_run, deadline, execute, failing, ledger_path, settle};
use crate::land::checks::Verdict;
use crate::land::lander::Lander;

/// The red verdict a lander stopped in the middle of its culprit search recorded for `landed`, read from the run's own log so the search resumes instead of the checks rerunning; None when that log holds no failure.
pub fn resume(lander: &Lander, landed: Landed) -> Option<Outcome> {
    let run = after_run(lander, &landed, None, Cancel::default(), deadline(&lander.repo, &ledger_path(), &landed.repository, &landed.head));
    let failing = failing(&run.log, &run.outcome);
    if failing.is_empty() {
        return None;
    }
    let verdict = Verdict::RedMain(execute::failed_check(&run.outcome).unwrap_or_else(|| format!("log: {}", run.log.display())));
    let log = run.log.clone();
    let search = (!settle::stale_red(&landed, &run)).then(|| Search { landed, run, failing: failing.clone() });
    Some(Outcome { verdict, search, failing, log, superseded: false })
}
