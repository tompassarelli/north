mod lock;
mod manifest;
mod registry;
mod review;
mod snapshot;
mod source;
#[cfg(test)]
mod tests;
mod trailer;

pub use registry::{PublicRegistries, Registry};
pub use review::{check_reasons, floor_run, hold_floor, refuse_young, run, run_at};
pub use trailer::trailer_lines;
pub(crate) use lock::locked_versions;
pub(crate) use snapshot::{Snapshot, git, read_snapshot};

use regex::Regex;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

const TRAILER_FORMAT: &str = "Dependency: <name> — <reason> (ASCII - also accepted)";
const URGENT_FORMAT: &str = "Dependency-Urgent: <name>@<version> <advisory-id>; flakes use node@locked.rev (otherwise locked.version or locked.narHash)";
type Parsed<T> = Result<T, &'static str>;
type Identity = (String, String, String);
type Urgent = BTreeSet<(String, String)>;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct Dependency {
    name: String,
    source: String,
    allowed: bool,
}

#[derive(Debug)]
struct LockCandidate {
    path: String,
    ecosystem: String,
    name: String,
    version: String,
    published: Option<f64>,
    owner: Option<String>,
}

impl LockCandidate {
    fn identity(&self) -> Identity {
        (
            self.ecosystem.clone(),
            self.name.clone(),
            self.version.clone(),
        )
    }
}

fn matches(pattern: &str, text: &str) -> bool {
    Regex::new(pattern)
        .expect("valid dependency expression")
        .is_match(text)
}

fn table(value: &Value) -> Parsed<&Map<String, Value>> {
    value.as_object().ok_or("ValueError")
}

fn string(value: &Value) -> Parsed<&str> {
    let text = value.as_str().ok_or("ValueError")?;
    if text.trim().is_empty() || text.chars().any(|c| c < ' ') {
        return Err("ValueError");
    }
    Ok(text.trim())
}

fn get<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}

fn optional_table<'a>(value: &'a Value, key: &str) -> Parsed<Option<&'a Map<String, Value>>> {
    value.get(key).map(table).transpose()
}

fn required<'a>(value: &'a Value, key: &str) -> Parsed<&'a Value> {
    value.get(key).ok_or("KeyError")
}

pub(crate) fn parse_toml(text: &str) -> Parsed<Value> {
    crate::platform::toml_json::parse(text).map_err(|_| "TOMLDecodeError")
}

fn parse_json(text: &str) -> Parsed<Value> {
    serde_json::from_str(text).map_err(|_| "JSONDecodeError")
}
