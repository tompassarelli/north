use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::layout::Layout;

/// What a lander that is replaced waits for before it leaves: the runtime it leaves for, the second its limit ends, and
/// the work it waits for, named for the probe's report.
pub struct Record {
    pub target: String,
    pub until: u64,
    pub what: String,
}

impl Record {
    pub fn open(&self) -> bool {
        now() < self.until
    }
}

fn path(layout: &Layout, pid: u32) -> PathBuf {
    layout.leaving().join(pid.to_string())
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

pub fn write(layout: &Layout, pid: u32, record: &Record) -> io::Result<()> {
    fs::create_dir_all(layout.leaving())?;
    fs::write(path(layout, pid), format!("{} {} {}\n", record.target, record.until, record.what))
}

pub fn remove(layout: &Layout, pid: u32) {
    let _ = fs::remove_file(path(layout, pid));
}

pub fn record(layout: &Layout, pid: u32) -> Option<Record> {
    let text = fs::read_to_string(path(layout, pid)).ok()?;
    let mut parts = text.trim().splitn(3, ' ');
    let target = parts.next()?.to_owned();
    let until = parts.next()?.parse().ok()?;
    let what = parts.next()?.to_owned();
    Some(Record { target, until, what })
}

pub fn target(layout: &Layout, pid: u32) -> Option<String> {
    record(layout, pid).filter(Record::open).map(|record| record.target)
}
