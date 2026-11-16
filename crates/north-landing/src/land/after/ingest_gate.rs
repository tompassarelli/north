use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use crate::land::telemetry::epoch;

const LOCK: &str = "follow-ingest-lock";
const SLOT: &str = "follow-ingest-slot";
const COVERED: &str = "follow-ingest-covered";

pub enum Turn {
    Covered { by: String },
    Ingest { heads: Vec<String> },
}

pub struct Held(File);

impl Held {
    /// Stamps the lock with the time this ingest ended, which the next ingest waits its pause from.
    pub fn ended(self) {
        let _ = self.0.set_len(0).and_then(|()| (&self.0).write_all(epoch().to_string().as_bytes()));
    }
}

fn open(git_dir: &Path, name: &str) -> Option<File> {
    OpenOptions::new().create(true).append(true).open(git_dir.join(name)).ok()
}

/// Settles whether this follow ingests: one ingest runs and at most one waits. A follow that finds the slot taken records its head beside the locks and is `Covered`; the one in the slot starts `pause` after the running ingest ends and gets the heads covered meanwhile with its own, whose crate facts it tells beside the full ingest, so every landed head keeps them.
pub fn take(git_dir: &Path, head: &str, pause: Duration) -> (Turn, Option<Held>) {
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
    let ended = fs::read_to_string(git_dir.join(LOCK)).ok().and_then(|stamp| stamp.trim().parse::<f64>().ok());
    if let Some(wait) = ended.map(|ended| ended + pause.as_secs_f64() - epoch()).filter(|wait| (0.0..=pause.as_secs_f64()).contains(wait)) {
        std::thread::sleep(Duration::from_secs_f64(wait));
    }
    let _ = covered.lock();
    let mut heads: Vec<String> = fs::read_to_string(git_dir.join(COVERED)).unwrap_or_default().lines().map(str::to_owned).collect();
    let _ = covered.set_len(0);
    drop(slot);
    let _ = covered.unlock();
    heads.push(head.to_owned());
    (Turn::Ingest { heads }, Some(Held(lock)))
}
