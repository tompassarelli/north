use std::fs;
use std::io;
use std::path::Path;

use crate::platform::disk_budget::DiskBudget;
use crate::platform::shared_target;

const USAGE: &str = "usage: north land-clean LANE_ROOT | --sweep";
const MIB: u64 = 1 << 20;

pub fn run(arguments: &[String]) -> u8 {
    if arguments == ["--sweep"] {
        return sweep_home_cache();
    }
    let [lane] = arguments else {
        eprintln!("{USAGE}");
        return 64;
    };
    match release(Path::new(lane)) {
        Ok(Some(bytes)) => println!("land-clean: removed {lane}/target ({} MiB)", bytes / MIB),
        Ok(None) => {}
        Err(error) => eprintln!("land-clean: could not remove {lane}/target: {error}"),
    }
    0
}

fn sweep_home_cache() -> u8 {
    let Some(home) = std::env::var_os("HOME") else {
        eprintln!("land-clean: cannot sweep: HOME is not set");
        return 1;
    };
    shared_target::run_sweep(&Path::new(&home).join(".cache"), shared_target::cap_bytes(&DiskBudget::local()))
}

pub fn release(lane: &Path) -> io::Result<Option<u64>> {
    let in_worktrees = lane.parent().and_then(Path::file_name).is_some_and(|name| name == "worktrees");
    let target = lane.join("target");
    let linked = fs::symlink_metadata(lane.join(".git")).is_ok_and(|git| git.is_file());
    let cargo_output = fs::symlink_metadata(&target).is_ok_and(|target| target.is_dir())
        && target.join("CACHEDIR.TAG").is_file();
    if !(in_worktrees && linked && cargo_output) {
        return Ok(None);
    }
    let bytes = size(&target)?;
    fs::remove_dir_all(&target)?;
    Ok(Some(bytes))
}

fn size(directory: &Path) -> io::Result<u64> {
    let mut total = 0;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        total += if metadata.is_dir() { size(&entry.path())? } else { metadata.len() };
    }
    Ok(total)
}

#[cfg(test)]
#[path = "lane_target_tests.rs"]
mod tests;
