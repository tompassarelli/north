pub mod category;
pub mod check_log;
pub mod landing;
pub mod profile;
pub mod report;

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

pub use category::Category;

pub const SPANS_ENV: &str = "NORTH_LAND_SPANS";
pub const PARENT_ENV: &str = "NORTH_LAND_SPAN_PARENT";
pub const PHASE: &str = "phase";
pub const AFTER: &str = "after";
pub const COVERED_BY: &str = "covered_by";
pub const VERDICT: &str = "verdict";
const FILE: &str = "spans.jsonl";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub trace: String,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub name: String,
    pub category: Category,
    pub start: f64,
    pub end: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub host: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu: Option<f64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
}

impl Span {
    pub fn new(trace: &str, name: impl Into<String>, category: Category, start: f64, end: f64) -> Self {
        Self { trace: trace.to_owned(), id: fresh_id(), parent: None, name: name.into(), category, start, end: end.max(start), host: String::new(), cpu: None, attributes: BTreeMap::new() }
    }

    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    #[must_use]
    pub fn under(mut self, parent: &str) -> Self {
        self.parent = Some(parent.to_owned());
        self
    }

    #[must_use]
    pub fn on(mut self, host: &str) -> Self {
        host.clone_into(&mut self.host);
        self
    }

    #[must_use]
    pub fn cpu(mut self, cpu: Option<f64>) -> Self {
        self.cpu = cpu;
        self
    }

    #[must_use]
    pub fn with(mut self, key: &str, value: impl Into<String>) -> Self {
        self.attributes.insert(key.to_owned(), value.into());
        self
    }

    pub fn seconds(&self) -> f64 {
        self.end - self.start
    }

    pub fn attribute(&self, key: &str) -> Option<&str> {
        self.attributes.get(key).map(String::as_str)
    }
}

fn fresh_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let micros = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_micros());
    format!("{micros:x}-{:x}-{:x}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

pub fn check_id(head: &str) -> String {
    format!("check:{head}")
}

pub fn this_host() -> String {
    fs::read_to_string("/proc/sys/kernel/hostname").map_or_else(|_| "local".to_owned(), |name| name.trim().to_owned())
}

pub struct Spans {
    path: PathBuf,
}

impl Spans {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn beside(logs: &Path) -> Self {
        Self::at(logs.join(FILE))
    }

    pub fn from_env() -> Option<Self> {
        std::env::var_os(SPANS_ENV).filter(|value| !value.is_empty()).map(Self::at)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&self, spans: &[Span]) -> io::Result<()> {
        if spans.is_empty() {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut text = String::new();
        for span in spans {
            text.push_str(&serde_json::to_string(span).map_err(io::Error::other)?);
            text.push('\n');
        }
        OpenOptions::new().create(true).append(true).open(&self.path)?.write_all(text.as_bytes())
    }

    pub fn read(&self) -> Vec<Span> {
        fs::read_to_string(&self.path).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
    }
}

pub fn record(spans: &[Span]) {
    if let Some(sink) = Spans::from_env()
        && let Err(error) = sink.append(spans)
    {
        eprintln!("north land: could not record spans in {}: {error}", sink.path().display());
    }
}
