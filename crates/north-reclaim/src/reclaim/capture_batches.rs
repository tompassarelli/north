use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::Path;
use std::time::Duration;

use crate::platform::disk_budget::Share;

use super::held::Held;
use super::{Inputs, Policy, Removal, hours, idle_for, removed, size};

const DEFAULT_BYTES: u64 = 15 << 30;
const MIN_AGE: Duration = Duration::from_secs(24 * 3600);

struct Batch {
    name: String,
    path: std::path::PathBuf,
    bytes: u64,
    idle: Duration,
}

/// Deletes the oldest capture-queue batches whose jobs all have results until the store fits its share; a batch with a queued or running job, a pin, a holder or a change within a day stays.
pub(super) fn sweep(inputs: &Inputs, policy: &Policy, held: &Held) -> io::Result<Vec<Removal>> {
    let batches_dir = inputs.capture_queue.join("batches");
    let entries = match fs::read_dir(&batches_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let cap = inputs.disk_budget.bytes(Share::CaptureBatches).unwrap_or(DEFAULT_BYTES);
    let mut batches = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let path = entry.path();
            let idle = idle_for(&path, inputs.now).ok()?;
            Some(Batch { name: entry.file_name().to_string_lossy().into_owned(), bytes: size(&path), idle, path })
        })
        .collect::<Vec<_>>();
    let mut total = batches.iter().map(|batch| batch.bytes).sum::<u64>();
    if total <= cap {
        return Ok(Vec::new());
    }
    let finished = finished_batches(&inputs.capture_queue.join("results"));
    let waiting = waiting_jobs(&inputs.capture_queue);
    batches.retain(|batch| {
        batch.idle >= MIN_AGE
            && finished.contains(&batch.name)
            && !held.within(&batch.path)
            && !batch.path.join("PINNED").exists()
            && !runs_a_waiting_job(&batch.path, &waiting)
    });
    batches.sort_by_key(|batch| std::cmp::Reverse(batch.idle));
    let mut removals = Vec::new();
    for batch in batches {
        if total <= cap {
            break;
        }
        if policy.dry_run || removed(&batch.path) {
            total -= batch.bytes;
            let reason = format!("capture batch idle {}h with its results kept, oldest past the {} GiB share", hours(batch.idle), cap >> 30);
            removals.push(Removal { path: batch.path, reason, bytes: batch.bytes });
        }
    }
    Ok(removals)
}

fn finished_batches(results: &Path) -> BTreeSet<String> {
    let Ok(entries) = fs::read_dir(results) else { return BTreeSet::new() };
    entries
        .flatten()
        .filter_map(|entry| fs::read_to_string(entry.path().join("result.json")).ok())
        .filter_map(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter_map(|result| result.get("batch")?.as_str().map(str::to_owned))
        .collect()
}

fn waiting_jobs(queue: &Path) -> BTreeSet<String> {
    ["pending", "taken"]
        .iter()
        .filter_map(|part| fs::read_dir(queue.join(part)).ok())
        .flat_map(|entries| entries.flatten())
        .filter_map(|entry| entry.file_name().to_string_lossy().strip_suffix(".json").map(str::to_owned))
        .collect()
}

fn runs_a_waiting_job(batch: &Path, waiting: &BTreeSet<String>) -> bool {
    let Ok(entries) = fs::read_dir(batch) else { return false };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("pads-"))
        .filter_map(|pads| fs::read_dir(pads.path()).ok())
        .flat_map(|jobs| jobs.flatten())
        .any(|job| waiting.contains(job.file_name().to_string_lossy().as_ref()))
}
