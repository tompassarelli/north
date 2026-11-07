use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use toml_edit::{DocumentMut, Item};

use super::{Range, git};

const REGISTRY: &str = "harness/gates.toml";
const SELF: &str = "gate-shadow";
const REVIEW_DECISIONS: i64 = 20;
const REVIEW_HOURS: i64 = 24;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Gate {
    pub enforcing: bool,
    pub reviewed: bool,
}

fn review_reads_enough(review: &Item) -> bool {
    let number = |key: &str| review.get(key).and_then(Item::as_integer).unwrap_or(0);
    let named = |key: &str| review.get(key).and_then(Item::as_str).is_some_and(|text| !text.trim().is_empty());
    named("reviewed") && named("by") && (number("decisions") >= REVIEW_DECISIONS || number("hours") >= REVIEW_HOURS)
}

pub fn gates(text: &str) -> BTreeMap<String, Gate> {
    let Ok(document) = text.parse::<DocumentMut>() else {
        return BTreeMap::new();
    };
    let Some(table) = document.get("gate").and_then(Item::as_table) else {
        return BTreeMap::new();
    };
    table
        .iter()
        .map(|(name, entry)| {
            let enforcing = entry.get("mode").and_then(Item::as_str) == Some("enforce");
            let reviewed = entry.get("review").is_some_and(review_reads_enough);
            (name.to_owned(), Gate { enforcing, reviewed })
        })
        .collect()
}

/// The gates `head` makes enforce that `base` did not enforce and whose `[gate.NAME.review]` does
/// not record a reviewer, a date and 24 h or 20 decisions of shadow reading.
pub fn unreviewed(base: &BTreeMap<String, Gate>, head: &BTreeMap<String, Gate>) -> Vec<String> {
    head.iter()
        .filter(|(name, gate)| gate.enforcing && !gate.reviewed && !base.get(*name).is_some_and(|before| before.enforcing))
        .map(|(name, _)| name.clone())
        .collect()
}

fn registry_at(directory: &Path, revision: &str) -> BTreeMap<String, Gate> {
    git::text(directory, &["show", &format!("{revision}:{REGISTRY}")]).map(|text| gates(&text)).unwrap_or_default()
}

fn record(state: &Path, head: &str, refused: &[String]) {
    let directory = state.join("north/gates");
    let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let line = serde_json::json!({"gate": SELF, "at": at, "head": head, "would_refuse": !refused.is_empty(), "gates": refused});
    let _ = std::fs::create_dir_all(&directory);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(directory.join("gate-shadow.jsonl")) {
        let _ = file.write_all(format!("{line}\n").as_bytes());
    }
}

pub fn scan(directory: &Path, range: &str, state: &Path, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        let _ = writeln!(out, "gate-shadow: expected BASE..HEAD, got {range}");
        return 2;
    };
    let fork = git::merge_base(directory, base, head);
    let before = registry_at(directory, &fork);
    let after = registry_at(directory, head);
    let refused = unreviewed(&before, &after);
    let enforcing = [&before, &after].iter().any(|registry| registry.get(SELF).is_some_and(|gate| gate.enforcing));
    if !enforcing {
        record(state, head, &refused);
    }
    if refused.is_empty() {
        return 0;
    }
    let verb = if enforcing { "refused" } else { "would refuse (shadow)" };
    let _ = writeln!(out, "gate-shadow: {verb}: a gate turns on enforcement without a `[gate.NAME.review]` in {REGISTRY} naming `reviewed`, `by` and at least {REVIEW_HOURS} `hours` or {REVIEW_DECISIONS} `decisions` of shadow decisions read; a new or changed gate lands in `mode = \"shadow\"` first:");
    for name in &refused {
        let _ = writeln!(out, "  {name}");
    }
    u8::from(enforcing)
}

#[cfg(test)]
#[path = "gate_shadow_tests.rs"]
mod tests;
