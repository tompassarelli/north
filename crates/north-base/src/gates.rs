//! The gate registry, harness/gates.toml: each gate that can refuse a launch, a push, a landing or a GitHub write and
//! was added since north#706, with its mode. A gate in shadow logs every decision and what it would refuse, and lets
//! the action through; it enforces only once a reviewer has read its would-be refusals over 24 h or 20 decisions and
//! recorded that review in its entry (docs/rationale/shadow-first-gates.md).
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use toml_edit::{DocumentMut, Item};

use crate::foundation::clock::Timestamp;

pub const PATH: &str = "harness/gates.toml";

/// Where a new file is a gate: the guards, the landing scans, the diff gates and safe-push's gates.
pub const DIRECTORIES: [&str; 4] = ["crates/north-guard/src/guard/", "crates/north-landing/src/land/scans/", "crates/north-diff-gates/src/", "crates/north-safe-push/src/gates/"];

pub const MIN_DECISIONS: u64 = 20;
pub const MIN_SECONDS: i64 = 24 * 3_600;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Shadow,
    Enforce,
}

/// A reviewer's reading of a gate's shadow log: the span read, its decisions, how many it would have refused and how
/// many of those were wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Review {
    pub reviewer: String,
    pub from: i64,
    pub to: i64,
    pub decisions: u64,
    pub would_refuse: u64,
    pub wrong: u64,
}

impl Review {
    /// Why this review does not yet let the gate enforce, or None when it does.
    pub fn short(&self, now: i64) -> Option<String> {
        if self.reviewer.trim().is_empty() {
            return Some("its review names no reviewer".into());
        }
        if self.to > now || self.from >= self.to {
            return Some("its review span is not a past interval".into());
        }
        if self.decisions < MIN_DECISIONS && self.to - self.from < MIN_SECONDS {
            return Some(format!("its review read {} decisions over {:.1} h, under {MIN_DECISIONS} decisions or 24 h", self.decisions, (self.to - self.from) as f64 / 3_600.0));
        }
        if self.wrong > 0 {
            return Some(format!("its review found {} wrong would-be refusals", self.wrong));
        }
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gate {
    pub id: String,
    pub files: Vec<String>,
    pub mode: Mode,
    pub review: Option<Review>,
}

fn text(item: &Item, key: &str) -> Option<String> {
    item.get(key).and_then(Item::as_str).map(str::to_owned)
}

fn moment(item: &Item, key: &str, id: &str) -> Result<i64, String> {
    let stamp = text(item, key).ok_or_else(|| format!("{id}: review.{key} is missing"))?;
    Timestamp::parse_loose(&stamp).map(Timestamp::unix_seconds).ok_or_else(|| format!("{id}: review.{key} {stamp} is no UTC time"))
}

fn count(item: &Item, key: &str) -> u64 {
    item.get(key).and_then(Item::as_integer).and_then(|value| u64::try_from(value).ok()).unwrap_or(0)
}

fn review(item: &Item, id: &str) -> Result<Review, String> {
    Ok(Review { reviewer: text(item, "reviewer").unwrap_or_default(), from: moment(item, "from", id)?, to: moment(item, "to", id)?, decisions: count(item, "decisions"), would_refuse: count(item, "would_refuse"), wrong: count(item, "wrong") })
}

/// The gates a registry text declares.
pub fn parse(text_of: &str) -> Result<Vec<Gate>, String> {
    let document: DocumentMut = text_of.parse().map_err(|why| format!("{PATH}: {why}"))?;
    let Some(gates) = document.get("gate") else { return Ok(Vec::new()) };
    let gates = gates.as_array_of_tables().ok_or_else(|| format!("{PATH}: gate is not an array of tables"))?;
    gates
        .iter()
        .map(|table| {
            let item = Item::Table(table.clone());
            let id = text(&item, "id").filter(|id| !id.is_empty()).ok_or_else(|| format!("{PATH}: a gate has no id"))?;
            let mode = match text(&item, "mode").as_deref() {
                Some("shadow") => Mode::Shadow,
                Some("enforce") => Mode::Enforce,
                other => return Err(format!("{id}: mode {other:?} is neither shadow nor enforce")),
            };
            let files = item.get("files").and_then(Item::as_array).map(|files| files.iter().filter_map(|file| file.as_str().map(str::to_owned)).collect()).unwrap_or_default();
            let review = item.get("review").map(|found| review(found, &id)).transpose()?;
            Ok(Gate { id, files, mode, review })
        })
        .collect()
}

static REGISTRY: LazyLock<Vec<Gate>> = LazyLock::new(|| parse(include_str!("../../../harness/gates.toml")).expect("harness/gates.toml parses"));

fn log_path(state: &Path, id: &str) -> PathBuf {
    state.join("north/gates").join(format!("{id}.jsonl"))
}

fn state_home() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"), PathBuf::from)
}

/// The refusal a registered gate makes of one decision: the refusal itself when it enforces, None in shadow, where
/// the decision and what it would refuse go to the gate's shadow log. A gate not in the registry enforces.
pub fn decide(id: &str, refusal: Option<String>) -> Option<String> {
    let mode = REGISTRY.iter().find(|gate| gate.id == id).map_or(Mode::Enforce, |gate| gate.mode);
    if mode == Mode::Enforce {
        return refusal;
    }
    let path = log_path(&state_home(), id);
    let row = serde_json::json!({
        "at": Timestamp::now().iso(),
        "gate": id,
        "would_refuse": refusal.is_some(),
        "reason": refusal.as_deref().unwrap_or_default(),
        "session": std::env::var("NORTH_FLEET_SESSION").unwrap_or_default(),
    });
    let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::OpenOptions::new().create(true).append(true).open(&path)).and_then(|mut file| writeln!(file, "{row}"));
    if let Err(why) = written {
        eprintln!("{id} (shadow): could not log to {}: {why}", path.display());
    }
    if let Some(reason) = &refusal {
        eprintln!("{id} (shadow): would refuse: {reason}");
    }
    None
}
