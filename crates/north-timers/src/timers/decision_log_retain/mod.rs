//! `north ops strategy log retain`: keeps this host's decision log to its retention for the hourly user timer:
//! closed hours compressed with zstd, days past 14 deleted, the open hour left for appends.
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::foundation::clock::Timestamp;
use crate::platform::decision_log;

#[cfg(test)]
mod tests;

const KEEP_DAYS: i64 = 14;

pub fn run(arguments: &[String]) -> u8 {
    if *arguments != ["log", "retain"] {
        eprintln!("usage: north ops strategy log retain");
        return 2;
    }
    let Some(root) = decision_log::root() else {
        eprintln!("north ops strategy log retain: the decision log is off (NORTH_DECISION_LOG is empty)");
        return 2;
    };
    let retained = retain(&root, decision_log::now(), KEEP_DAYS);
    println!("north ops strategy log retain: {retained}");
    u8::from(retained.failed)
}

#[derive(Debug, Default)]
struct Retained {
    days_deleted: usize,
    hours_compressed: usize,
    hours_left_uncompressed: usize,
    failed: bool,
}

impl std::fmt::Display for Retained {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let counts = [(self.days_deleted, "days deleted"), (self.hours_compressed, "hours compressed"), (self.hours_left_uncompressed, "hours left uncompressed")];
        let shown: Vec<String> = counts.iter().filter(|(count, _)| *count > 0).map(|(count, what)| format!("{count} {what}")).collect();
        out.write_str(if shown.is_empty() { "nothing to do".to_owned() } else { shown.join(", ") }.as_str())
    }
}

/// Deletes each `KIND/YYYY-MM-DD` directory older than `days` and compresses the closed hours of the rest.
fn retain(root: &Path, now: f64, days: i64) -> Retained {
    let stamp = Timestamp::from_unix_seconds_f64(now);
    let open_hour = format!("{}.", &stamp.iso()[11..13]);
    let mut retained = Retained::default();
    for day in entries(root).iter().flat_map(|kind| entries(kind)) {
        let name = file_name(&day);
        let Some(start) = Timestamp::parse_loose(&format!("{name}T00:00:00Z")) else { continue };
        if stamp.unix_days() - start.unix_days() > days {
            retained.failed |= std::fs::remove_dir_all(&day).is_err();
            retained.days_deleted += 1;
        } else {
            compress_closed(&day, (name == stamp.iso_date()).then_some(open_hour.as_str()), &mut retained);
        }
    }
    retained
}

/// Compresses each `HH.jsonl` and `HH.snapshots.jsonl` of `day` with zstd, except the open hour's.
fn compress_closed(day: &Path, open_hour: Option<&str>, retained: &mut Retained) {
    for file in entries(day) {
        let name = file_name(&file);
        if !name.ends_with(".jsonl") || open_hour.is_some_and(|hour| name.starts_with(hour)) {
            continue;
        }
        if Command::new("zstd").args(["-q", "-f", "--rm", "-19"]).arg(&file).status().is_ok_and(|status| status.success()) {
            retained.hours_compressed += 1;
        } else {
            retained.hours_left_uncompressed += 1;
            retained.failed = true;
        }
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

fn entries(directory: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(directory).into_iter().flatten().flatten().map(|entry| entry.path()).collect();
    found.sort();
    found
}
