//! The reuse a build system records itself (docs/rationale/compile-reuse.md, Tests): Buck2 answers a test binary whose
//! action already passed from its action cache, so each test binary is one unit, reused when its result came from there.
use std::io::BufReader;
use std::process::{Command, Stdio};

use north_build_system::{TestRecord, TestRun};

use north_base::verdict_cache::reuse_log::Record;
use super::outcome::parse;

pub fn read(record: &TestRecord) -> Result<Vec<TestRun>, String> {
    let mut child = Command::new("bash").args(["-euo", "pipefail", "-c", &record.command]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|error| format!("{}: {error}", record.command))?;
    let runs = child.stdout.take().map_or_else(|| Ok(Vec::new()), |printed| record.runs(&mut BufReader::new(printed)));
    if runs.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().map_err(|error| format!("{}: {error}", record.command))?;
    if runs.is_ok() && !status.success() {
        return Err(format!("{} exited {status}", record.command));
    }
    runs
}

pub fn units(runs: &[TestRun], probe: &str) -> Vec<Record> {
    runs.iter().map(|run| Record::Unit { probe: probe.to_owned(), unit: run.target.clone(), set: run.action.clone().filter(|_| run.reused), passed: run.passed, seconds: run.seconds }).collect()
}

/// The cases the test binaries printed and how many of them did not pass.
pub fn cases(runs: &[TestRun]) -> (usize, usize) {
    let outcomes: Vec<_> = runs.iter().flat_map(|run| parse(&run.printed)).collect();
    (outcomes.len(), outcomes.iter().filter(|outcome| !outcome.passed).count())
}
