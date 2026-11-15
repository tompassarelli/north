use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::layout::Layout;

fn path(layout: &Layout, pid: u32) -> PathBuf {
    layout.leaving().join(pid.to_string())
}

pub fn write(layout: &Layout, pid: u32, target: &str, until: u64) -> io::Result<()> {
    fs::create_dir_all(layout.leaving())?;
    fs::write(path(layout, pid), format!("{target} {until}\n"))
}

pub fn remove(layout: &Layout, pid: u32) {
    let _ = fs::remove_file(path(layout, pid));
}

pub fn target(layout: &Layout, pid: u32) -> Option<String> {
    let text = fs::read_to_string(path(layout, pid)).ok()?;
    let (target, until) = text.trim().split_once(' ')?;
    let until: u64 = until.parse().ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    (now < until).then(|| target.to_owned())
}
