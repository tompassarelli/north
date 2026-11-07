use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use crate::error::NorthResult;
use crate::fleet::model::{SessionRecord, Status};
use crate::fleet::records::record_file;
use crate::fleet::stranded::reported_done;
use crate::fleet::transcript::stream_path;

use super::stranding::recent;
use super::idle_exit;
use super::Fleet;

impl Fleet {
    /// The recently active sessions a reboot at `booted_ms` stranded: not stopped, host process
    /// gone, last written before the boot, mid-turn or within its idle limit at the boot (a host
    /// idle longer had parked itself, which is no reboot's doing) and not ended on a `Done:` report.
    /// Reviving one writes its record again, so a later run lists it no more.
    pub fn rebooted(&self, booted_ms: u64) -> NorthResult<Vec<String>> {
        let mut names = Vec::new();
        for record in self.sessions()?.into_iter().filter(recent) {
            if self.status(&record) != Status::Parked || !written_before(&self.session_dir(&record.name)?, booted_ms) {
                continue;
            }
            let mid_turn = self.observed_status(&record).is_ok_and(|status| status == Status::Working);
            if !mid_turn && !alive_at(&record, booted_ms) {
                continue;
            }
            if self.last_report(&record).is_some_and(|(report, _)| reported_done(&report)) {
                continue;
            }
            names.push(record.name);
        }
        Ok(names)
    }
}

fn alive_at(record: &SessionRecord, booted_ms: u64) -> bool {
    let Some(limit) = idle_exit(record) else { return true };
    let wrote = fs::metadata(stream_path(&record.host_dir)).and_then(|meta| meta.modified()).ok();
    let idle = wrote.and_then(|at| (UNIX_EPOCH + Duration::from_millis(booted_ms)).duration_since(at).ok());
    idle.is_none_or(|idle| idle < limit)
}

fn written_before(session_dir: &std::path::Path, booted_ms: u64) -> bool {
    let written = fs::metadata(record_file(session_dir)).and_then(|meta| meta.modified()).ok();
    written.is_some_and(|at| at < UNIX_EPOCH + Duration::from_millis(booted_ms))
}
