//! Each landing's rework verdict against one checkout's main, judged once and kept beside the checkout's history cache.
//!
//! A verdict depends only on the commits within its window on main, so once main has moved past that window it never
//! changes and is never judged again. An open verdict holds for the main head it was judged at, so a read at that head
//! that names no new landing reads no history and judges nothing.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::history;
use super::rework::{self, Landing, Verdict};

const FILE: &str = "north-rework.json";

/// The verdicts judged so far against one main: `judged` holds every landing asked about at `head`, and a landing
/// in it without a verdict is not on that main.
#[derive(Default, Serialize, Deserialize)]
struct Store {
    head: String,
    judged: BTreeSet<String>,
    verdicts: BTreeMap<String, Verdict>,
}

impl Store {
    fn settled(&self, key: &str, head: &str) -> bool {
        self.verdicts.get(key).is_some_and(|verdict| verdict.closed) || (self.head == head && self.judged.contains(key))
    }
}

/// A landing's item takes part in its verdict, since a later commit can fix it forward by naming the item.
fn key(landing: &Landing) -> String {
    format!("{} {}", landing.sha, landing.item.as_deref().unwrap_or_default())
}

/// The verdict of each of `landings` that the checkout's main holds, by its index in `landings`, judging only
/// those no stored verdict settles and reading the history from `since` only when there are any.
pub fn verdicts(checkout: &Path, landings: &[Landing], since: Option<i64>) -> Vec<(usize, Verdict)> {
    let (Some(directory), Some(head)) = (history::git_dir(checkout), history::head(checkout)) else { return Vec::new() };
    let path = directory.join(FILE);
    let mut store = read(&path).unwrap_or_default();
    let keys: Vec<String> = landings.iter().map(key).collect();
    let pending: Vec<usize> = (0..landings.len()).filter(|&index| !store.settled(&keys[index], &head)).collect();
    if !pending.is_empty() {
        let Some(commits) = history::history(checkout, since) else { return Vec::new() };
        let asked: Vec<&Landing> = pending.iter().map(|&index| &landings[index]).collect();
        for &index in &pending {
            store.verdicts.remove(&keys[index]);
        }
        for (asked_index, verdict) in rework::judge_history(&commits, &asked) {
            store.verdicts.insert(keys[pending[asked_index]].clone(), verdict);
        }
        store.head = head;
        store.judged = keys.iter().cloned().collect();
        write(&path, &store);
    }
    keys.iter().enumerate().filter_map(|(index, key)| Some((index, store.verdicts.get(key)?.clone()))).collect()
}

fn read(path: &Path) -> Option<Store> {
    serde_json::from_reader(BufReader::new(File::open(path).ok()?)).ok()
}

/// Writes `store` atomically; a failed write only costs the next read a fresh judgment.
fn write(path: &Path, store: &Store) {
    let written = path.parent().and_then(|directory| tempfile::NamedTempFile::new_in(directory).ok()).and_then(|file| {
        let mut writer = BufWriter::new(file);
        serde_json::to_writer(&mut writer, store).ok()?;
        writer.flush().ok()?;
        writer.into_inner().ok()?.persist(path).ok()
    });
    if written.is_none() {
        let _ = fs::remove_file(path);
    }
}
