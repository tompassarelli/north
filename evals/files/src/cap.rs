use std::collections::BTreeSet;

use crate::task::Tree;

pub const FILE_BYTES: u64 = 8192;
pub const TOTAL_BYTES: u64 = 32768;

pub enum Dropped {
    OverFile,
    OverTotal,
}

pub fn apply(ranked: &[String], tree: &Tree) -> (Vec<String>, Vec<(String, Dropped)>) {
    let (mut kept, mut dropped, mut total) = (Vec::new(), Vec::new(), 0);
    for path in ranked {
        let Some(&size) = tree.sizes.get(path) else { continue };
        if size > FILE_BYTES {
            dropped.push((path.clone(), Dropped::OverFile));
        } else if total + size > TOTAL_BYTES {
            dropped.push((path.clone(), Dropped::OverTotal));
        } else {
            total += size;
            kept.push(path.clone());
        }
    }
    (kept, dropped)
}

pub fn reachable(visible: &BTreeSet<String>, truth: &BTreeSet<String>, tree: &Tree) -> usize {
    truth.iter().filter(|path| visible.contains(*path) && tree.sizes.get(*path).is_some_and(|&size| size <= FILE_BYTES)).count()
}

pub fn union_in_order(parts: &[&[String]]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    parts.iter().flat_map(|part| part.iter()).filter(|path| seen.insert((*path).clone())).cloned().collect()
}

pub fn interleave(parts: &[&[String]]) -> Vec<String> {
    let longest = parts.iter().map(|part| part.len()).max().unwrap_or(0);
    let mut seen = BTreeSet::new();
    (0..longest).flat_map(|i| parts.iter().filter_map(move |part| part.get(i))).filter(|path| seen.insert((*path).clone())).cloned().collect()
}
