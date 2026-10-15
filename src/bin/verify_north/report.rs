use std::io::{self, Write};
use std::time::Duration;

pub fn step(number: &str, title: &str, outcome: &Result<String, String>, elapsed: Duration) {
    let (verdict, detail) = match outcome {
        Ok(detail) => ("ok  ", detail),
        Err(reason) => ("FAIL", reason),
    };
    let separator = if detail.is_empty() { "" } else { ": " };
    say(&format!("step {number} {verdict} {:>7} ms  {title}{separator}{detail}", elapsed.as_millis()));
}

pub fn skipped(number: &str, title: &str) {
    say(&format!("step {number} skip {:>7} ms  {title}", 0));
}

pub fn total(failed: Option<&str>, elapsed: Duration) {
    let verdict = failed.map_or_else(|| "passed".to_owned(), |number| format!("FAILED at step {number}"));
    say(&format!("verify-north {verdict}  total {} ms", elapsed.as_millis()));
}

pub fn say(line: &str) {
    let mut stdout = io::stdout().lock();
    let _ = writeln!(stdout, "{line}").and_then(|()| stdout.flush());
}
