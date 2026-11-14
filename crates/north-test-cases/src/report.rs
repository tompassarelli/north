use std::io::Write;
use std::time::Duration;

use crate::run::{CASE_LIMIT, Outcome, Verdict};

pub const FAILED: u8 = 100;
pub const BROKEN: u8 = 101;

pub fn print(binary_id: &str, outcomes: &[Outcome], ignored: usize, took: Duration) -> bool {
    let mut out = std::io::stdout().lock();
    let mut failed = 0;
    for outcome in outcomes {
        let status = match outcome.verdict {
            Verdict::Pass => "PASS".to_owned(),
            Verdict::Fail => "FAIL".to_owned(),
            Verdict::Signal(signal) => signal_name(signal),
            Verdict::Overran => "TIMEOUT".to_owned(),
        };
        _ = writeln!(out, "{status:>12} [{:>8.3}s] {binary_id} {}", outcome.took.as_secs_f64(), outcome.name);
        if !matches!(outcome.verdict, Verdict::Pass) {
            failed += 1;
            if matches!(outcome.verdict, Verdict::Overran) {
                _ = writeln!(out, "{binary_id} {} ran over its {} s limit and was killed", outcome.name, CASE_LIMIT.as_secs());
            }
            _ = out.write_all(&outcome.output);
        }
    }
    let passed = outcomes.len() - failed;
    _ = writeln!(out, "{:>12} [{:>8.3}s] {} tests run: {passed} passed, {failed} failed, {ignored} skipped", "Summary", took.as_secs_f64(), outcomes.len());
    failed == 0
}

fn signal_name(signal: i32) -> String {
    let name = match signal {
        4 => "ILL",
        6 => "ABRT",
        7 => "BUS",
        8 => "FPE",
        9 => "KILL",
        11 => "SEGV",
        15 => "TERM",
        _ => return format!("SIG{signal}"),
    };
    format!("SIG{name}")
}
