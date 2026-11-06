//! Trims sccache's local store to its cap, least recently used entry first. sccache stamps an entry's mtime on every hit, so the oldest
//! mtime is the entry least likely to be asked for again; a removed entry is a cache miss its next compile refills.
use std::env;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::platform::disk_budget::{DiskBudget, Share};

const DEFAULT_CAP_BYTES: u64 = 10 << 30;

/// The store the lanes' sccache writes: `SCCACHE_DIR`, else sccache's own default under the home's cache.
pub fn root(home: &Path) -> PathBuf {
    env::var_os("SCCACHE_DIR").filter(|dir| !dir.is_empty()).map_or_else(|| home.join(".cache/sccache"), PathBuf::from)
}

/// The host's `sccache` disk budget, else the `SCCACHE_CACHE_SIZE` sccache itself trims to, else sccache's 10 GiB default.
pub fn cap_bytes(budget: &DiskBudget) -> u64 {
    budget.bytes(Share::CompilerCache).or_else(|| env::var("SCCACHE_CACHE_SIZE").ok().and_then(|size| parse_size(&size))).unwrap_or(DEFAULT_CAP_BYTES)
}

/// sccache's size syntax: a whole number with an optional K, M, G or T suffix, in powers of 1024.
pub fn parse_size(text: &str) -> Option<u64> {
    let text = text.trim();
    let (digits, shift) = match text.chars().last()?.to_ascii_uppercase() {
        'K' => (&text[..text.len() - 1], 10),
        'M' => (&text[..text.len() - 1], 20),
        'G' => (&text[..text.len() - 1], 30),
        'T' => (&text[..text.len() - 1], 40),
        _ => (text, 0),
    };
    digits.parse::<u64>().ok()?.checked_mul(1 << shift)
}

/// Removes the oldest entries under `root` until it holds at most `cap` bytes; the bytes freed.
pub fn trim(root: &Path, cap: u64) -> u64 {
    let mut entries = Vec::new();
    collect(root, &mut entries);
    let mut held: u64 = entries.iter().map(|(_, bytes, _)| bytes).sum();
    if held <= cap {
        return 0;
    }
    entries.sort_by_key(|(touched, _, _)| *touched);
    let mut freed = 0;
    for (_, bytes, path) in entries {
        if held <= cap {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            held = held.saturating_sub(bytes);
            freed += bytes;
        }
    }
    freed
}

fn collect(directory: &Path, entries: &mut Vec<(SystemTime, u64, PathBuf)>) {
    let Ok(listing) = fs::read_dir(directory) else { return };
    for entry in listing.flatten() {
        let Ok(metadata) = entry.metadata() else { continue };
        if metadata.is_dir() {
            collect(&entry.path(), entries);
        } else if metadata.is_file() {
            let touched = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            entries.push((touched, metadata.len().max(metadata.blocks() * 512), entry.path()));
        }
    }
}
