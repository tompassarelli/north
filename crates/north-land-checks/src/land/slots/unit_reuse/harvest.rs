use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use north_base::platform::records_mirror::state_home;

use super::lines::{MARK, Record};

const DAY: u64 = 86_400;
const KEPT_DAYS: u64 = 14;

pub fn directory() -> PathBuf {
    state_home().join("north/land-reuse")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

fn day_file(directory: &Path, epoch: u64) -> PathBuf {
    directory.join(format!("{}.tsv", epoch / DAY))
}

pub fn harvest(log: &Path, directory: &Path) {
    let Ok(text) = fs::read_to_string(log) else { return };
    let marked = format!("{MARK}\t");
    let (shadow, kept): (Vec<&str>, Vec<&str>) = text.lines().partition(|line| line.starts_with(&marked));
    if shadow.is_empty() {
        return;
    }
    let epoch = now();
    let block: String = shadow.iter().filter_map(|line| Record::parse(line.strip_prefix(&marked)?)).map(|record| format!("{epoch}\t{}\n", record.render())).collect();
    if fs::create_dir_all(directory).is_ok() && let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(day_file(directory, epoch)) {
        let _ = file.write_all(block.as_bytes());
    }
    let _ = fs::write(log, kept.iter().map(|line| format!("{line}\n")).collect::<String>());
    prune(directory, epoch);
}

fn prune(directory: &Path, epoch: u64) {
    let oldest = (epoch / DAY).saturating_sub(KEPT_DAYS);
    for entry in fs::read_dir(directory).into_iter().flatten().filter_map(Result::ok) {
        let day = entry.file_name().to_str().and_then(|name| name.strip_suffix(".tsv")).and_then(|day| day.parse::<u64>().ok());
        if day.is_some_and(|day| day < oldest) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

pub fn read_since(directory: &Path, since: u64) -> Vec<(u64, Record)> {
    let mut found = Vec::new();
    for day in (since / DAY)..=(now() / DAY) {
        let Ok(text) = fs::read_to_string(day_file(directory, day * DAY)) else { continue };
        for line in text.lines() {
            let Some((epoch, rest)) = line.split_once('\t') else { continue };
            if let (Ok(epoch), Some(record)) = (epoch.parse::<u64>(), Record::parse(rest))
                && epoch >= since
            {
                found.push((epoch, record));
            }
        }
    }
    found
}
