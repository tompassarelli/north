use super::super::layout::Layout;
use super::super::pin::now;
use std::fs;
use std::path::{Path, PathBuf};

/// verify-north's report lines (src/bin/verify_north/report.rs and journey.rs): the step it failed, that step's report line, and the directory it kept its scratch ledger in.
const FAILED_STEP: &str = "verify-north failed step ";
const FAIL: &str = " FAIL ";
const KEPT_LEDGER: &str = "verify-north kept the scratch ledger in ";
const KEPT_LOG_LINES: usize = 200;

/// Keeps a failed spine walk's evidence in `verify-north/failed/STAMP`, the directory verify-north kept its scratch ledger in when its log names one: the failing step's name, its report line and the tail of the log; none when the log holds no failed step. On 2026-11-11 that directory was empty and the follow log held no step text, so nobody could tell the runtime's fault from the worker's.
pub(super) fn failure(layout: &Layout, log: &str) -> Option<PathBuf> {
    let lines: Vec<&str> = log.lines().map(str::trim_end).collect();
    let report = lines.iter().rev().find(|line| line.starts_with("step ") && line.contains(FAIL))?;
    let step = lines.iter().rev().find_map(|line| line.strip_prefix(FAILED_STEP)).map_or_else(|| report.split(FAIL).next().unwrap_or_default().to_owned(), |named| format!("step {named}"));
    let stamp = lines.iter().rev().find_map(|line| line.strip_prefix(KEPT_LEDGER)).and_then(|kept| Path::new(kept).file_name()).map_or_else(|| now().to_string(), |stamp| stamp.to_string_lossy().into_owned());
    let kept = layout.state.join("verify-north/failed").join(stamp);
    let tail = lines[lines.len().saturating_sub(KEPT_LOG_LINES)..].join("\n");
    let written = fs::create_dir_all(&kept).and_then(|()| fs::write(kept.join("step"), format!("{step}\n"))).and_then(|()| fs::write(kept.join("report"), format!("{report}\n"))).and_then(|()| fs::write(kept.join("log"), format!("{tail}\n")));
    match written {
        Ok(()) => Some(kept),
        Err(error) => {
            println!("north runtime: could not keep the failed spine walk in {}: {error}", kept.display());
            None
        }
    }
}
