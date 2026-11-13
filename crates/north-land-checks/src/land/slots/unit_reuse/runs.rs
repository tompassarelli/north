//! The reuse a build system records itself (docs/rationale/compile-reuse.md, Tests): Buck2 answers a test binary whose
//! action already passed from its action cache, so each test binary is one unit, reused when its result came from there.
use north_build_system::TestRun;

use north_base::verdict_cache::reuse_log::Record;
use super::outcome::parse;

pub fn units(runs: &[TestRun], probe: &str) -> Vec<Record> {
    runs.iter().map(|run| Record::Unit { probe: probe.to_owned(), unit: run.target.clone(), set: run.action.clone().filter(|_| run.reused), passed: run.passed, seconds: run.seconds }).collect()
}

/// The cases the test binaries printed and how many of them did not pass.
pub fn cases(runs: &[TestRun]) -> (usize, usize) {
    let outcomes: Vec<_> = runs.iter().flat_map(|run| parse(&run.printed)).collect();
    (outcomes.len(), outcomes.iter().filter(|outcome| !outcome.passed).count())
}
