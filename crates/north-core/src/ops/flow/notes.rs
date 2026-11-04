//! The caveats printed under the flow rows: how many finished worker briefs carry every facet and whether the landing watch is alive.
use std::time::Duration;

use serde_json::Value;

use super::host::Places;
use crate::platform::sqlite::{self, Access};

const CATEGORIES: [&str; 11] = [
    "mechanical", "docs-policy", "tooling", "feature", "bug-known-cause", "debugging-unknown-cause", "netcode-determinism", "performance", "balance-tuning", "native-check", "research",
];
const FACETS: [(&str, &[&str]); 4] = [
    ("Spec", &["exact", "measured", "judged"]),
    ("Scope", &["one-file", "module", "cross-module", "cross-repo"]),
    ("Surface", &["ts", "rust", "lua", "nix", "shell", "workflow", "docs", "assets"]),
    ("Verify", &["none", "local-test", "farm", "native", "visual"]),
];
const ORIGINS: [&str; 2] = ["codex.PostToolUse.spawn_agent", "claude.PostToolUse.Agent"];

fn field<'a>(brief: &'a str, name: &str) -> Option<&'a str> {
    brief.lines().find_map(|line| {
        let start = line.find(&format!("{name}:"))?;
        let rest = line[start + name.len() + 1..].trim_start_matches([' ', '\t']);
        let end = rest.find(|character: char| !(character.is_ascii_alphabetic() || character == '-')).unwrap_or(rest.len());
        (end > 0).then_some(&rest[..end])
    })
}

fn covered(row: &Value) -> bool {
    let brief = row["brief_text"].as_str().unwrap_or_default();
    let category = row["category"].as_str().unwrap_or_default();
    let declared = field(brief, "Category").map(str::to_lowercase);
    CATEGORIES.contains(&category)
        && declared.as_deref() == Some(category)
        && FACETS.iter().all(|(name, allowed)| {
            let value = row[name.to_lowercase()].as_str().unwrap_or_default();
            allowed.contains(&value) && field(brief, name).map(str::to_lowercase).as_deref() == Some(value)
        })
        && row["brief_origin"].as_str().is_some_and(|origin| ORIGINS.contains(&origin))
}

/// How many finished worker runs recorded a brief naming every facet, `None` when the ledger cannot be read.
pub fn brief_coverage(places: &Places) -> Option<usize> {
    let query = "SELECT category,spec,scope,surface,verify,brief_text,brief_origin FROM runs WHERE role='worker' AND outcome != 'running'";
    let rows = sqlite::rows(&places.ledger, Access::ReadOnly, Duration::from_secs(10), query).ok()?;
    Some(rows.iter().filter(|row| covered(row)).count())
}

pub fn watch_silent(places: &Places, now: f64, limit: f64) -> Option<Option<f64>> {
    let beat = std::fs::read_to_string(places.state.join("north/watch/heartbeat")).ok().and_then(|text| text.trim().parse::<f64>().ok());
    match beat {
        Some(at) if now - at <= limit => None,
        Some(at) => Some(Some(now - at)),
        None => Some(None),
    }
}
