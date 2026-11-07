use std::io::Write;
use std::path::Path;

use serde_json::{Deserializer, Value};
use sha2::{Digest, Sha256};

use super::{Range, git};

const POLICY: &str = "agents/AGENTS.md";
const TRIALS: u64 = 3;

fn replay_entries(directory: &Path, head: &str, suffix: &str) -> Vec<String> {
    git::text(
        directory,
        &["ls-tree", "--name-only", head, "--", "agents/replay/"],
    )
    .unwrap_or_default()
    .lines()
    .filter_map(|entry| {
        let name = entry.strip_prefix("agents/replay/")?;
        (!name.contains('/') && name.len() > suffix.len() && name.ends_with(suffix))
            .then(|| entry.to_owned())
    })
    .collect()
}

fn scenario_ids(directory: &Path, head: &str) -> Vec<String> {
    replay_entries(directory, head, ".json")
        .iter()
        .filter_map(|file| {
            let json = git::bytes(directory, &["show", &format!("{head}:{file}")])?;
            let stem = file.strip_prefix("agents/replay/")?.strip_suffix(".json")?;
            Some(
                match serde_json::from_slice::<Value>(&json).ok()?.get("id") {
                    None | Some(Value::Null | Value::Bool(false)) => stem.to_owned(),
                    Some(Value::String(id)) => id.clone(),
                    Some(id) => id.to_string(),
                },
            )
        })
        .collect()
}

fn trials(row: &Value, scenario: &str) -> u64 {
    row.get("scenarios")
        .and_then(|scenarios| scenarios.get(scenario))
        .and_then(|counts| counts.get(1))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

fn swept(directory: &Path, head: &str, sha: &str, wanted: &[String]) -> bool {
    let mut rows = Vec::new();
    for file in replay_entries(directory, head, ".jsonl")
        .iter()
        .filter(|file| file.starts_with("agents/replay/sweep-"))
    {
        let Some(text) = git::bytes(directory, &["show", &format!("{head}:{file}")]) else {
            return false;
        };
        for row in Deserializer::from_slice(&text).into_iter::<Value>() {
            let Ok(row) = row else { return false };
            rows.push(row);
        }
    }
    rows.iter().any(|row| {
        row.get("policy_sha256").and_then(Value::as_str) == Some(sha)
            && wanted
                .iter()
                .all(|scenario| trials(row, scenario) >= TRIALS)
    })
}

pub fn scan(directory: &Path, range: &str, out: &mut impl Write) -> u8 {
    let Some(Range { base, head }) = Range::parse(range) else {
        return 0;
    };
    let blob = |side: &str| {
        git::text(
            directory,
            &["rev-parse", "-q", "--verify", &format!("{side}:{POLICY}")],
        )
    };
    let Some(live) = blob(head) else { return 0 };
    if blob(base) == Some(live) {
        return 0;
    }
    let policy = git::bytes(directory, &["show", &format!("{head}:{POLICY}")]).unwrap_or_default();
    let sha: String = Sha256::digest(&policy)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    if swept(directory, head, &sha, &scenario_ids(directory, head)) {
        return 0;
    }
    let _ = writeln!(
        out,
        "safe-push: ✗ this change makes {POLICY} {short} live and the tree has no agents/replay/sweep-*.jsonl row for it with {TRIALS} trials on every scenario, so every host's agents sync would refuse it ('the live policy has no sweep row') and roll its runtime update back. Run the sweep in this worktree and commit the row it writes: north policy-replay sweep agents/replay/*.json --variants agents/replay/variants.toml --variant base --trials 3 (about 9 minutes and $2 at list price: the 2026-10-27 sweeps took 480-567 s and $1.35-$2.23 for 42 runs); NOT pushed.",
        short = &sha[..8]
    );
    1
}
