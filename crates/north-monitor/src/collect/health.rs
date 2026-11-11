use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Deserialize;

use crate::snapshot::{Builds, Health};

const TAIL_BYTES: u64 = 3_000_000;
const NEW_FAILURE_S: u64 = 600;

#[derive(Deserialize)]
struct Check {
    failing: bool,
    #[serde(default)]
    since: Option<u64>,
}

#[derive(Deserialize)]
struct HealthFile {
    #[serde(default)]
    checks: BTreeMap<String, Check>,
}

pub fn checks(state: &Path) -> Result<Health, String> {
    let path = state.join("health.json");
    let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let written = fs::metadata(&path).ok().and_then(|meta| meta.modified().ok()).and_then(|time| time.duration_since(UNIX_EPOCH).ok()).map_or(0, |since| since.as_secs());
    let file: HealthFile = serde_json::from_str(&text).map_err(|error| format!("health.json: {error}"))?;
    let mut health = Health { source: "health.json checks".into(), written, ..Health::default() };
    for (name, check) in file.checks {
        match (check.failing, check.since.unwrap_or(0) + NEW_FAILURE_S > written) {
            (false, _) => health.green += 1,
            (true, true) => health.amber.push(name),
            (true, false) => health.red.push(name),
        }
    }
    Ok(health)
}

pub fn ladder(state: &Path) -> Result<u8, String> {
    let path = state.join("ladder-level");
    match fs::read_to_string(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(format!("{}: {error}", path.display())),
        Ok(text) => {
            let word = text.trim().trim_start_matches(['L', 'l']);
            word.parse::<u8>().ok().filter(|level| *level <= 4).ok_or_else(|| format!("ladder-level holds {:?}, not L0 to L4", text.trim()))
        }
    }
}

#[derive(Deserialize)]
struct Span {
    #[serde(default)]
    trace: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    category: String,
    start: f64,
    end: f64,
}

fn spans_file(state: &Path) -> Option<PathBuf> {
    fs::read_dir(state.join("land-queue"))
        .ok()?
        .flatten()
        .map(|repo| repo.path().join("logs/spans.jsonl"))
        .filter_map(|path| Some((fs::metadata(&path).ok()?.modified().ok()?, path)))
        .max()
        .map(|(_, path)| path)
}

pub fn builds_tail(state: &Path) -> Result<Builds, String> {
    let path = spans_file(state).ok_or("no land-queue/*/logs/spans.jsonl")?;
    let mut file = File::open(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let length = file.metadata().map_err(|error| error.to_string())?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(TAIL_BYTES))).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    Ok(builds_of(&spans(&String::from_utf8_lossy(&bytes)), f64::MAX))
}

pub fn builds_all(state: &Path) -> Result<Vec<SpanLite>, String> {
    let path = spans_file(state).ok_or("no land-queue/*/logs/spans.jsonl")?;
    let text = fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(spans(&text))
}

pub struct SpanLite {
    trace: String,
    kind: Kind,
    start: f64,
    end: f64,
}

#[derive(PartialEq)]
enum Kind {
    LandChecks,
    Compile,
    Test,
}

fn spans(text: &str) -> Vec<SpanLite> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Span>(line).ok())
        .filter_map(|span| {
            let kind = match (span.category.as_str(), span.name.as_str()) {
                ("lander", "land checks") => Kind::LandChecks,
                ("compile", "cargo build") => Kind::Compile,
                ("test", _) => Kind::Test,
                _ => return None,
            };
            Some(SpanLite { trace: span.trace, kind, start: span.start, end: span.end })
        })
        .collect()
}

pub fn builds_of(spans: &[SpanLite], until: f64) -> Builds {
    let Some(last) = spans.iter().filter(|span| span.kind == Kind::LandChecks && span.end <= until).max_by(|left, right| left.end.total_cmp(&right.end)) else {
        return Builds::default();
    };
    let longest = |kind: Kind| spans.iter().filter(|span| span.trace == last.trace && span.kind == kind && span.end <= until).map(|span| span.end - span.start).reduce(f64::max);
    Builds { compile_s: longest(Kind::Compile), test_s: longest(Kind::Test), land_checks_s: Some(last.end - last.start), ended_at: Some(last.end as u64) }
}
