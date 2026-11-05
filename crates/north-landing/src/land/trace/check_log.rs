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
const SLOTS_BUSY: &str = "slots busy for ";
const RESERVED: &str = "\"decision\":\"RESERVED\"";

pub fn is_marker(line: &str) -> bool {
    let line = line.trim();
    line.starts_with(RUNNING) || line.starts_with(DOC_TESTS) || line.starts_with(FINISHED) || line.starts_with(TEST_RESULT) || line.starts_with(OFFLOAD_RUNNING) || line.contains(SLOTS_BUSY) || line.contains(RESERVED)
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
    let mut busy: Option<(f64, f64)> = None;
    let mut test: Option<(String, f64)> = None;
    for mark in marks {
        let text = mark.line.trim();
        if text.contains(SLOTS_BUSY) {
            busy = Some((busy.map_or(mark.at, |(since, _)| since), mark.at));
            continue;
        }
        if let Some((since, last)) = busy.take() {
            spans.push(child("offload slot wait", Category::HostWait, since, last));
        }
        if text.contains(RESERVED) {
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
    if let Some((since, last)) = busy {
        spans.push(child("offload slot wait", Category::HostWait, since, last));
    }
    spans
}

#[cfg(test)]
#[path = "check_log_tests.rs"]
mod check_log_tests;
