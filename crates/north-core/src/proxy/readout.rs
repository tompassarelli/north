//! `north proxy calls TRANSCRIPT`: the main-thread tool calls of a Claude Code transcript, counted
//! by kind, with the streaks between hand-offs and the counted shapes run 20 or more times.
use std::collections::{BTreeMap, HashSet};

use serde_json::Value;

use super::calls::{Kind, VerbKinds, classify};
use super::shape::shape;

const REPEAT_SHAPE: usize = 20;
const WARN_STREAK: usize = 5;
const DENY_STREAK: usize = 15;

#[derive(Debug, Default, Eq, PartialEq)]
pub struct Readout {
    pub kinds: Vec<Kind>,
    pub shapes: Vec<(Kind, String)>,
    pub sidechain: usize,
}

pub fn read(jsonl: &str, verbs: &VerbKinds) -> Readout {
    let mut readout = Readout::default();
    let mut seen = HashSet::new();
    for entry in jsonl.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()) {
        if entry.get("type").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let sidechain = entry.get("isSidechain").and_then(Value::as_bool).unwrap_or(false);
        let blocks = entry.pointer("/message/content").and_then(Value::as_array).into_iter().flatten();
        for block in blocks.filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use")) {
            if !block.get("id").and_then(Value::as_str).is_none_or(|id| seen.insert(id.to_owned())) {
                continue;
            }
            if sidechain {
                readout.sidechain += 1;
                continue;
            }
            let tool = block.get("name").and_then(Value::as_str).unwrap_or_default();
            let input = block.get("input").cloned().unwrap_or(Value::Null);
            let kind = classify(tool, &input, verbs);
            readout.kinds.push(kind);
            readout.shapes.push((kind, shape(tool, &input)));
        }
    }
    readout
}

pub fn streaks(kinds: &[Kind]) -> Vec<usize> {
    kinds.split(|kind| kind.hands_work_away()).map(<[Kind]>::len).filter(|len| *len > 0).collect()
}

fn counted(kind: Kind) -> bool {
    matches!(kind, Kind::Diagnosis | Kind::LandingLogs | Kind::Status | Kind::Monitoring | Kind::Bookkeeping)
}

pub fn render(readout: &Readout) -> String {
    let mut lines = vec![format!("calls: {} (subagent calls skipped: {})", readout.kinds.len(), readout.sidechain)];
    let mut by_kind: BTreeMap<Kind, usize> = BTreeMap::new();
    for kind in &readout.kinds {
        *by_kind.entry(*kind).or_default() += 1;
    }
    let mut ranked: Vec<(Kind, usize)> = Kind::ALL.iter().map(|kind| (*kind, by_kind.get(kind).copied().unwrap_or(0))).collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    lines.extend(ranked.iter().map(|(kind, count)| format!("- {}: {count}", kind.label())));
    let mut runs = streaks(&readout.kinds);
    runs.sort_unstable();
    let rank = |share: usize| runs.get((runs.len() * share).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0);
    lines.push(format!(
        "streaks: {}, median {}, 90th percentile {}, longest {}; {} reached {WARN_STREAK}, {} reached {DENY_STREAK}",
        runs.len(),
        rank(50),
        rank(90),
        runs.last().copied().unwrap_or(0),
        runs.iter().filter(|len| **len >= WARN_STREAK).count(),
        runs.iter().filter(|len| **len >= DENY_STREAK).count(),
    ));
    let mut shapes: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, shape) in readout.shapes.iter().filter(|(kind, _)| counted(*kind)) {
        *shapes.entry(shape).or_default() += 1;
    }
    let mut repeated: Vec<(&str, usize)> = shapes.into_iter().filter(|(_, count)| *count >= REPEAT_SHAPE).collect();
    repeated.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    lines.push(format!("counted shapes run {REPEAT_SHAPE} or more times: {}", repeated.len()));
    lines.extend(repeated.iter().map(|(shape, count)| format!("- {shape}: {count}")));
    lines.join("\n")
}
