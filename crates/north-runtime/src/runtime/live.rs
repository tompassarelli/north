use super::layout::{self, Layout};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) const SWAPS_KEPT: usize = 200;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub(super) struct Swap {
    pub(super) runtime: String,
    pub(super) at: i64,
    #[serde(skip)]
    pub(super) estimated: bool,
}

pub(super) fn note_swap(layout: &Layout, link: &Path) {
    let Some(runtime) = layout::linked_sha(link) else { return };
    let at = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs() as i64);
    let Ok(line) = serde_json::to_string(&Swap { runtime: runtime.to_owned(), at, estimated: false }) else { return };
    let _ = fs::OpenOptions::new().create(true).append(true).open(layout.swaps()).and_then(|mut file| writeln!(file, "{line}"));
}
