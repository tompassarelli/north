use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crate::platform::disk_budget::{DiskBudget, Share};

use super::held::Held;
use super::{Inputs, Policy, Removal, hours, idle_for, removed, setting, size};

const GIB: f64 = (1u64 << 30) as f64;

pub(super) struct Budget {
    pub(super) bytes: u64,
    pub(super) min_age: Duration,
}

impl Budget {
    pub(super) fn of(budget: &DiskBudget) -> Self {
        let bytes = budget.bytes(Share::SmashcraftEvidence).unwrap_or((setting("evidence_budget_gib") * GIB) as u64);
        Self { bytes, min_age: Duration::from_secs_f64(setting("evidence_min_age_hours") * 3600.0) }
    }
}

struct Run {
    path: PathBuf,
    name: String,
    bytes: u64,
    idle: Option<Duration>,
}

/// Deletes the oldest evidence runs until the store, with `incoming` more bytes, fits the budget.
pub(super) fn sweep(inputs: &Inputs, policy: &Policy, held: &Held, budget: &Budget, incoming: u64) -> io::Result<Vec<Removal>> {
    let entries = match fs::read_dir(&inputs.evidence) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut runs = entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            Run { name: entry.file_name().to_string_lossy().into_owned(), bytes: size(&path), idle: idle_for(&path, inputs.now).ok(), path }
        })
        .collect::<Vec<_>>();
    let mut total = runs.iter().map(|run| run.bytes).sum::<u64>() + incoming;
    if total <= budget.bytes {
        return Ok(Vec::new());
    }
    let linked = match (inputs.open_issue_text)() {
        Ok(text) => linked_runs(&text),
        Err(error) => {
            eprintln!("north reclaim: evidence holds {} MiB over its {} MiB budget, but open issues are unreadable ({}), so no run is deleted", total >> 20, budget.bytes >> 20, error.trim());
            return Ok(Vec::new());
        }
    };
    runs.retain(|run| {
        run.idle.is_some_and(|idle| idle >= budget.min_age) && !linked.contains(&run.name) && !held.within(&run.path) && !run.path.join("PINNED").exists()
    });
    runs.sort_by_key(|run| std::cmp::Reverse(run.idle));
    let mut removals = Vec::new();
    for run in runs {
        if total <= budget.bytes {
            break;
        }
        if policy.dry_run || removed(&run.path) {
            total -= run.bytes;
            let reason = format!("evidence run idle {}h, oldest past the {} GiB budget", hours(run.idle.unwrap_or_default()), budget.bytes >> 30);
            removals.push(Removal { path: run.path, reason, bytes: run.bytes });
        }
    }
    Ok(removals)
}

/// Every run name an `evidence/<run>` mention in the text names.
fn linked_runs(text: &str) -> BTreeSet<String> {
    text.match_indices("evidence/")
        .map(|(at, prefix)| text[at + prefix.len()..].chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')).collect::<String>())
        .flat_map(|name| [name.trim_end_matches('.').to_owned(), name])
        .filter(|name| !name.is_empty())
        .collect()
}

/// The bodies and comments of the open issues in every catalogued repository that writes or cites Smashcraft evidence.
pub(super) fn open_issue_text() -> Result<String, String> {
    let owner = &crate::threads::catalog::CATALOG.owner;
    ["smashcraft", "north", "wisp"]
        .iter()
        .map(|repo| crate::platform::github::json(&["issue", "list", "-R", &format!("{owner}/{repo}"), "--state", "open", "--limit", "500", "--json", "body,comments"]).map(|issues| issues.to_string()))
        .collect::<Result<Vec<_>, _>>()
        .map(|texts| texts.join("\n"))
}
