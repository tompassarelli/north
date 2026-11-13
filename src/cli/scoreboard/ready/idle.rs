//! The longest stretch each Claude account sat idle, read from the pace history: a stretch is a run of pace rows with
//! nothing admitted to a worker, joined while the rows stay within `PACE_GAP_S` of each other.
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};

use serde::Deserialize;

use super::bar::DAYS;
use crate::cli::scoreboard::{Window, state_root};

const PACE_HISTORY: &str = "agents/pace-control.jsonl";
const PACE_GAP_S: f64 = 600.0;

#[derive(Deserialize)]
struct PaceRow {
    account: Option<String>,
    at: f64,
    #[serde(default)]
    admits: bool,
    #[serde(default)]
    workers: u32,
}

pub(super) struct Idle {
    stretches: Vec<(i64, i64)>,
}

impl Idle {
    pub(super) fn read(since: i64) -> Self {
        let samples = fs::File::open(state_root().join(PACE_HISTORY)).ok().and_then(|file| samples(file, since)).unwrap_or_default();
        Self { stretches: samples.into_values().flat_map(|mut series| stretches(&mut series)).collect() }
    }

    pub(super) fn longest_minutes(&self, window: Window) -> f64 {
        self.stretches.iter().filter(|&&(from, to)| to >= window.start && from < window.end).map(|&(from, to)| (to.min(window.end - 1) - from.max(window.start)).max(0)).max().unwrap_or(0) as f64 / 60.0
    }
}

/// The idle flags of each `claude:` account's pace rows at or after `since`, read from the newest part of the history.
fn samples(mut file: fs::File, since: i64) -> Option<HashMap<String, Vec<(f64, bool)>>> {
    let length = file.metadata().map_or(0, |meta| meta.len());
    let wanted = DAYS as u64 * 8 * 1_048_576;
    let start = length.saturating_sub(wanted);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut lines = BufReader::new(file).split(b'\n');
    if start > 0 {
        lines.next();
    }
    let mut samples: HashMap<String, Vec<(f64, bool)>> = HashMap::new();
    for (account, at, idle) in lines.map_while(Result::ok).filter_map(|line| pace_row(&line, since)) {
        samples.entry(account).or_default().push((at, idle));
    }
    Some(samples)
}

fn pace_row(line: &[u8], since: i64) -> Option<(String, f64, bool)> {
    if !line.windows(10).any(|window| window == b"\"account\":") && !line.windows(11).any(|window| window == b"\"account\": ") {
        return None;
    }
    let row = serde_json::from_slice::<PaceRow>(line).ok()?;
    let account = row.account.filter(|account| account.starts_with("claude:"))?;
    (row.at >= since as f64).then_some((account, row.at, row.admits && row.workers == 0))
}

/// The idle stretches of one account, as the first and last whole second of each.
fn stretches(series: &mut [(f64, bool)]) -> Vec<(i64, i64)> {
    series.sort_by(|left, right| left.0.total_cmp(&right.0));
    let mut stretches = Vec::new();
    let mut open: Option<(f64, f64)> = None;
    for (at, idle) in series.iter().copied() {
        open = match (idle, open) {
            (true, Some((from, last))) if at - last <= PACE_GAP_S => Some((from, at)),
            (true, Some((from, last))) => {
                stretches.push((from as i64, last as i64));
                Some((at, at))
            }
            (true, None) => Some((at, at)),
            (false, Some((from, last))) => {
                stretches.push((from as i64, last as i64));
                None
            }
            (false, None) => None,
        };
    }
    if let Some((from, last)) = open {
        stretches.push((from as i64, last as i64));
    }
    stretches
}
