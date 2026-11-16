use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

const LOCK: &str = "follow-ingest-lock";
const SLOT: &str = "follow-ingest-slot";
const COVERED: &str = "follow-ingest-covered";

pub enum Turn {
    Covered { by: String },
    Ingest { heads: Vec<String> },
}

fn open(git_dir: &Path, name: &str) -> Option<File> {
    OpenOptions::new().create(true).append(true).open(git_dir.join(name)).ok()
}

/// Settles whether this follow ingests: one ingest runs and at most one waits. A follow that finds the slot taken records its head beside the locks and is `Covered`; the one in the slot starts when the running ingest ends and gets the heads covered meanwhile with its own, whose crate facts it tells beside the full ingest, so every landed head keeps them.
pub fn take(git_dir: &Path, head: &str) -> (Turn, Option<File>) {
    let (Some(slot), Some(lock), Some(covered)) = (open(git_dir, SLOT), open(git_dir, LOCK), open(git_dir, COVERED)) else { return (Turn::Ingest { heads: vec![head.to_owned()] }, None) };
    let _ = covered.lock();
    if slot.try_lock().is_err() {
        let _ = (&covered).write_all(format!("{head}\n").as_bytes());
        let by = fs::read_to_string(git_dir.join(SLOT)).unwrap_or_default();
        return (Turn::Covered { by }, None);
    }
    let _ = slot.set_len(0).and_then(|()| (&slot).write_all(head.as_bytes()));
    let _ = covered.unlock();
    let _ = lock.lock();
    let _ = covered.lock();
    let mut heads: Vec<String> = fs::read_to_string(git_dir.join(COVERED)).unwrap_or_default().lines().map(str::to_owned).collect();
    let _ = covered.set_len(0);
    drop(slot);
    let _ = covered.unlock();
    heads.push(head.to_owned());
    (Turn::Ingest { heads }, Some(lock))
}
