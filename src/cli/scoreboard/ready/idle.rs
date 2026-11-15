//! The longest stretch each Claude account sat idle, read from the pace history: a stretch is a run of pace rows with
//! nothing admitted to a worker, joined while the rows stay within `PACE_GAP_S` of each other. Each run folds only the
//! rows appended since the last one into the stretches it saved.
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::bar::DAYS;
use crate::cli::scoreboard::{Window, state_root};

const PACE_HISTORY: &str = "agents/pace-control.jsonl";
const FOLDED: &str = "north/ready-idle.json";
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

#[derive(Default, Deserialize, Serialize)]
struct Folded {
    inode: u64,
    offset: u64,
    since: i64,
    accounts: HashMap<String, Account>,
}

#[derive(Default, Deserialize, Serialize)]
struct Account {
    closed: Vec<(i64, i64)>,
    open: Option<(f64, f64)>,
}

pub(super) struct Idle {
    stretches: Vec<(i64, i64)>,
}

impl Idle {
    pub(super) fn read(since: i64) -> Self {
        let cache = state_root().join(FOLDED);
        let saved = fs::read(&cache).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let folded = fs::File::open(state_root().join(PACE_HISTORY)).ok().and_then(|file| fold(file, saved, since));
        let Some(folded) = folded else { return Self { stretches: Vec::new() } };
        save(&cache, &folded);
        Self { stretches: folded.accounts.into_values().flat_map(|account| account.closed.into_iter().chain(account.open.map(|(from, last)| (from as i64, last as i64)))).collect() }
    }

    pub(super) fn longest_minutes(&self, window: Window) -> f64 {
        self.stretches.iter().filter(|&&(from, to)| to >= window.start && from < window.end).map(|&(from, to)| (to.min(window.end - 1) - from.max(window.start)).max(0)).max().unwrap_or(0) as f64 / 60.0
    }
}

/// `saved` advanced by the complete pace rows appended after its offset, or rebuilt from the newest part of the history when the file was replaced, truncated or `since` reaches further back than `saved` holds.
fn fold(mut file: fs::File, saved: Option<Folded>, since: i64) -> Option<Folded> {
    let meta = file.metadata().ok()?;
    let saved = saved.filter(|saved| saved.inode == meta.ino() && saved.offset <= meta.len() && saved.since <= since);
    let fresh = saved.is_none();
    let mut folded = saved.unwrap_or_else(|| {
        let start = meta.len().saturating_sub(DAYS as u64 * 8 * 1_048_576);
        Folded { inode: meta.ino(), offset: start, since, accounts: HashMap::new() }
    });
    file.seek(SeekFrom::Start(folded.offset)).ok()?;
    let mut reader = BufReader::new(file);
    let mut line = Vec::new();
    if fresh && folded.offset > 0 {
        folded.offset += reader.read_until(b'\n', &mut line).ok()? as u64;
    }
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line).ok()?;
        if read == 0 || line.last() != Some(&b'\n') {
            break;
        }
        folded.offset += read as u64;
        if let Some((account, at, idle)) = pace_row(&line, since) {
            folded.accounts.entry(account).or_default().push(at, idle);
        }
    }
    folded.since = since;
    for account in folded.accounts.values_mut() {
        account.closed.retain(|&(_, to)| to >= since);
    }
    Some(folded)
}

fn pace_row(line: &[u8], since: i64) -> Option<(String, f64, bool)> {
    if !line.windows(10).any(|window| window == b"\"account\":") && !line.windows(11).any(|window| window == b"\"account\": ") {
        return None;
    }
    let row = serde_json::from_slice::<PaceRow>(line.trim_ascii_end()).ok()?;
    let account = row.account.filter(|account| account.starts_with("claude:"))?;
    (row.at >= since as f64).then_some((account, row.at, row.admits && row.workers == 0))
}

impl Account {
    fn push(&mut self, at: f64, idle: bool) {
        self.open = match (idle, self.open) {
            (true, Some((from, last))) if at - last <= PACE_GAP_S => Some((from, at)),
            (true, Some((from, last))) => {
                self.closed.push((from as i64, last as i64));
                Some((at, at))
            }
            (true, None) => Some((at, at)),
            (false, Some((from, last))) => {
                self.closed.push((from as i64, last as i64));
                None
            }
            (false, None) => None,
        };
    }
}

fn save(path: &Path, folded: &Folded) {
    let written = path.parent().and_then(|directory| fs::create_dir_all(directory).ok().and_then(|()| tempfile::NamedTempFile::new_in(directory).ok())).and_then(|file| {
        serde_json::to_writer(std::io::BufWriter::new(file.as_file()), folded).ok()?;
        file.persist(path).ok()
    });
    if written.is_none() {
        let _ = fs::remove_file(path);
    }
}
