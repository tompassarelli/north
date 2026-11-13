use std::io::{BufRead, BufReader, Read};

use super::{Category, Span};

#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub at: f64,
    pub line: String,
}

pub struct Line<'a> {
    pub trace: &'a str,
    pub id: &'a str,
    pub category: Category,
    pub start: f64,
    pub end: f64,
    pub host: &'a str,
}

const RUNNING: &str = "Running ";
const DOC_TESTS: &str = "Doc-tests ";
const FINISHED: &str = "Finished `";
const TEST_RESULT: &str = "test result: ";
const OFFLOAD_RUNNING: &str = "offload: running ";
const WAITED_IN_LINE: &str = "offload: waited ";
const LINE_KEPT: usize = 4 * 1024;
const RESERVED: &str = "\"decision\":\"RESERVED\"";
const RELEASED: &str = "\"decision\":\"RELEASED\"";

pub fn is_marker(line: &str) -> bool {
    let line = line.trim();
    line.starts_with(RUNNING) || line.starts_with(DOC_TESTS) || line.starts_with(FINISHED) || line.starts_with(TEST_RESULT) || line.starts_with(OFFLOAD_RUNNING) || line.starts_with(WAITED_IN_LINE) || is_reserved(line) || is_released(line)
}

pub fn waited_in_line(line: &str) -> Option<f64> {
    line.trim().strip_prefix(WAITED_IN_LINE).and_then(|rest| rest.split_once('s')).and_then(|(seconds, _)| seconds.parse::<f64>().ok())
}

/// The first wait in a slot line that `stream` reports, read to its end whatever bytes it carries, keeping at most `LINE_KEPT` bytes of any one line.
pub fn waited_in_stream(stream: impl Read) -> Option<f64> {
    let mut reader = BufReader::new(stream);
    let (mut found, mut line) = (None, Vec::with_capacity(LINE_KEPT));
    loop {
        let chunk = match reader.fill_buf() {
            Ok([]) => break,
            Ok(chunk) => chunk,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };
        let (taken, ended) = chunk.iter().position(|byte| *byte == b'\n').map_or((chunk.len(), false), |at| (at + 1, true));
        line.extend_from_slice(&chunk[..taken.min(LINE_KEPT.saturating_sub(line.len()))]);
        reader.consume(taken);
        if ended {
            found = found.or_else(|| waited_in_line(&String::from_utf8_lossy(&line)));
            line.clear();
        }
    }
    found.or_else(|| waited_in_line(&String::from_utf8_lossy(&line)))
}

pub fn is_offload_running(line: &str) -> bool {
    line.trim().starts_with(OFFLOAD_RUNNING)
}

pub fn is_reserved(line: &str) -> bool {
    line.contains(RESERVED)
}

pub fn is_released(line: &str) -> bool {
    line.contains(RELEASED)
}

pub fn cargo_seconds(text: &str) -> Option<f64> {
    let tail = text.rsplit_once(" in ")?.1.trim();
    let mut seconds = 0.0;
    for part in tail.split_whitespace() {
        if let Some(minutes) = part.strip_suffix('m') {
            seconds += minutes.parse::<f64>().ok()? * 60.0;
        } else if let Some(value) = part.strip_suffix('s') {
            seconds += value.parse::<f64>().ok()?;
        } else {
            return None;
        }
    }
    Some(seconds)
}

fn queued_seconds(line: &str) -> Option<f64> {
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    value.get("queuedSeconds")?.as_f64()
}

pub fn test_binary(line: &str) -> Option<String> {
    let line = line.trim();
    if let Some(name) = line.strip_prefix(DOC_TESTS) {
        return Some(format!("{} doc-tests", name.trim().replace('_', "-")));
    }
    let rest = line.strip_prefix(RUNNING)?;
    let inner = rest.rsplit_once('(')?.1.trim_end_matches(')');
    let file = inner.rsplit('/').next()?;
    let name = file.rsplit_once('-').map_or(file, |(name, _)| name);
    let source = rest.split_whitespace().find(|word| word.contains('/') || word.ends_with(".rs")).unwrap_or_default();
    Some(if source.starts_with("tests/") { format!("{name} ({source})") } else { name.replace('_', "-") })
}

pub fn children(line: &Line<'_>, marks: &[Mark]) -> Vec<Span> {
    let clamp = |at: f64| at.clamp(line.start, line.end);
    let child = |name: &str, category: Category, start: f64, end: f64| Span::new(line.trace, name, category, clamp(start), clamp(end)).under(line.id).on(line.host);
    let mut spans = Vec::new();
    let mut test: Option<(String, f64)> = None;
    for mark in marks {
        let text = mark.line.trim();
        if let Some(waited) = waited_in_line(text) {
            if mark.at - waited > line.start {
                spans.push(child("offload sync", Category::Offload, line.start, mark.at - waited));
            }
            spans.push(child("offload slot wait", Category::HostWait, mark.at - waited, mark.at));
        } else if is_reserved(text) {
            if let Some(queued) = queued_seconds(text).filter(|queued| *queued > 0.0) {
                spans.push(child("capacity lease wait", Category::HostWait, mark.at - queued, mark.at));
            }
        } else if text.starts_with(OFFLOAD_RUNNING) {
            let waited: f64 = spans.iter().filter(|span| span.category == Category::HostWait).map(Span::seconds).sum();
            let from = spans.iter().filter(|span| span.category == Category::HostWait).map(|span| span.end).fold(line.start, f64::max);
            if mark.at - from > 0.0 {
                spans.push(child("offload sync", Category::Offload, from, mark.at).with("waited", format!("{waited:.1}")));
            }
        } else if text.starts_with(FINISHED) {
            if line.category != Category::Lint
                && let Some(seconds) = cargo_seconds(text)
            {
                let profile = text.trim_start_matches(FINISHED).split('`').next().unwrap_or_default();
                spans.push(child("cargo build", Category::Compile, mark.at - seconds, mark.at).with("profile", profile));
            }
        } else if let Some(binary) = test_binary(text) {
            test = Some((binary, mark.at));
        } else if text.starts_with(TEST_RESULT)
            && let Some((binary, since)) = test.take()
        {
            spans.push(child(&format!("test {binary}"), Category::Test, since, mark.at).with("crate", binary).with("result", text.trim_start_matches(TEST_RESULT).split(';').next().unwrap_or_default()));
        }
    }
    spans
}

#[cfg(test)]
#[path = "check_log_tests.rs"]
mod check_log_tests;
