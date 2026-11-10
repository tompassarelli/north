//! The report's closing `Stop:` line, kept in `runs.stop` to be shown beside the run; no verdict reads it
//! (docs/rationale/attribution.md).
use std::sync::LazyLock;

use regex::Regex;

static STOP_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^\W*stop\s*:[^\n]*").unwrap());

/// The report's last `Stop:` line, which the 600-character report the ledger keeps loses when the report is long.
pub fn line(report: &str) -> Option<String> {
    STOP_LINE.find_iter(report).last().map(|found| found.as_str().trim().chars().take(200).collect())
}
