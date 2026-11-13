use std::io::{BufRead, BufReader, Read};

use north_base::foundation::offload::TIMED;

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
const SANDBOX_STEPS: [(&str, &str); 7] = [("scope", "sandbox start: scope"), ("capacity gate", "sandbox start: capacity gate"), ("derivation", "sandbox start: derivation"), ("nix eval", "sandbox start: nix eval"), ("gc root", "sandbox start: gc root"), ("target seed", "sandbox start: target seed"), ("nix develop", "sandbox start: nix develop")];
const SLOT_STEPS: [(&str, &str); 5] = [("receive", "slot and tree proof: receive"), ("fill", "slot and tree proof: fill"), ("slot", "slot and tree proof: slot line"), ("place", "slot and tree proof: place"), ("proof", "slot and tree proof: proof")];
pub const SANDBOX_OTHER: &str = "sandbox start: other";
pub const SLOT_SHIP: &str = "slot and tree proof: ship";

pub fn is_marker(line: &str) -> bool {
    let line = line.trim();
    line.starts_with(RUNNING) || line.starts_with(DOC_TESTS) || line.starts_with(FINISHED) || line.starts_with(TEST_RESULT) || line.starts_with(OFFLOAD_RUNNING) || line.starts_with(WAITED_IN_LINE) || line.starts_with(TIMED) || is_reserved(line) || is_released(line)
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

/// What a released lease's start spent before its subject ran, step by step under its trace name, and how long the subject then ran, as the lease's RELEASED line names them.
#[derive(Clone, Debug, PartialEq)]
pub struct LeaseStart {
    pub steps: Vec<(&'static str, f64)>,
    pub subject_seconds: Option<f64>,
}

pub fn lease_start(line: &str) -> Option<LeaseStart> {
    if !is_released(line) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let steps = value.get("start")?.as_array()?.iter().filter_map(|step| Some((named(&SANDBOX_STEPS, step.get(0)?.as_str()?).unwrap_or(SANDBOX_OTHER), step.get(1)?.as_f64()?))).collect();
    Some(LeaseStart { steps, subject_seconds: value.get("subjectSeconds").and_then(serde_json::Value::as_f64) })
}

/// The steps an offload run's timed line names under their trace names, each with its seconds; a step this lander does not know stays in the job's ship time.
pub fn slot_steps(line: &str) -> Option<Vec<(&'static str, f64)>> {
    let rest = line.trim().strip_prefix(TIMED)?;
    Some(rest.split_whitespace().filter_map(|word| word.split_once('=')).filter_map(|(name, millis)| Some((named(&SLOT_STEPS, name)?, millis.parse::<f64>().ok()? / 1000.0))).collect())
}

fn named(table: &[(&'static str, &'static str)], name: &str) -> Option<&'static str> {
    table.iter().find(|(step, _)| *step == name).map(|(_, traced)| *traced)
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
    let mut reserved_at = None;
    for mark in marks {
        let text = mark.line.trim();
        if let Some(waited) = waited_in_line(text) {
            if mark.at - waited > line.start {
                spans.push(child("offload sync", Category::Offload, line.start, mark.at - waited));
            }
            spans.push(child("offload slot wait", Category::HostWait, mark.at - waited, mark.at));
        } else if is_reserved(text) {
            reserved_at = Some(mark.at);
            if let Some(queued) = queued_seconds(text).filter(|queued| *queued > 0.0) {
                spans.push(child("capacity lease wait", Category::HostWait, mark.at - queued, mark.at));
            }
        } else if let Some(start) = lease_start(text) {
            let mut at = reserved_at.unwrap_or(line.start);
            for (name, seconds) in start.steps {
                spans.push(child(name, Category::Offload, at, at + seconds));
                at += seconds;
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
