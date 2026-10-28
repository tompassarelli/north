//! The caveats printed under the flow rows: sessions the Fleet cannot reach, a stale usage cache
//! and how many finished worker briefs carry every facet.
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use super::alerts::Node;
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

fn running_claude(node: &Node) -> Option<u64> {
    (node.provider == "claude" && node.active).then_some(node.pid?).filter(|pid| *pid != 0)
}

/// Sessions whose process inherited a parent's `CLAUDE_CODE_CHILD_SESSION`, which Claude writes no transcript for.
pub(crate) fn untranscribed(nodes: &[Node]) -> Vec<String> {
    nodes
        .iter()
        .filter(|node| {
            let Some(pid) = running_claude(node) else { return false };
            std::fs::read(format!("/proc/{pid}/environ")).is_ok_and(|environ| environ.split(|byte| *byte == 0).any(|item| item.starts_with(b"CLAUDE_CODE_CHILD_SESSION=")))
        })
        .map(|node| node.id.clone())
        .collect()
}

fn session_names(sessions: &Path) -> Vec<(Option<String>, Option<String>)> {
    let Ok(listing) = std::fs::read_dir(sessions) else { return Vec::new() };
    listing
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "json"))
        .filter_map(|entry| serde_json::from_str::<Value>(&std::fs::read_to_string(entry.path()).ok()?).ok())
        .map(|record| (record["sessionId"].as_str().map(str::to_owned), record["name"].as_str().map(str::to_owned)))
        .collect()
}

pub(crate) fn claude_sessions(places: &Places) -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR").map_or_else(|| places.home.join(".claude"), PathBuf::from).join("sessions")
}

/// Running Claude sessions Claude does not know by their Fleet name.
pub(crate) fn unnamed(nodes: &[Node], sessions: &Path) -> Vec<String> {
    let named = session_names(sessions);
    nodes
        .iter()
        .filter(|node| running_claude(node).is_some_and(|pid| Path::new(&format!("/proc/{pid}")).exists()))
        .filter(|node| !named.iter().any(|(session, name)| session.is_some() && *session == node.session && name.as_deref() == Some(node.id.as_str())))
        .map(|node| node.id.clone())
        .collect()
}

/// Hours since the Claude usage cache was written, when that is over `hours`.
pub(crate) fn usage_stale(places: &Places, now: f64, hours: f64) -> Option<f64> {
    let written = std::fs::metadata(places.state.join("agents/usage/claude-web.json")).ok()?.modified().ok()?;
    let written = written.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs_f64();
    let age = (now - written) / 3600.0;
    (age > hours).then_some(age)
}

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
pub(crate) fn brief_coverage(places: &Places) -> Option<usize> {
    let query = "SELECT category,spec,scope,surface,verify,brief_text,brief_origin FROM runs WHERE role='worker' AND outcome != 'running'";
    let rows = sqlite::rows(&places.ledger, Access::ReadOnly, Duration::from_secs(10), query).ok()?;
    Some(rows.iter().filter(|row| covered(row)).count())
}
