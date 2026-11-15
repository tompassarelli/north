//! The cgroup units that used the most CPU on this dispatch host over a short sample, printed by a dry run when the host's one-minute load is over twice its cores.
use super::survey::Survey;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const CGROUPS: &str = "/sys/fs/cgroup";
const SLICES: [&str; 2] = ["system.slice", "user.slice"];
const SAMPLE: Duration = Duration::from_secs(5);
const TOP: usize = 10;
const DEPTH: usize = 6;

/// Prints the ten largest CPU consumers by unit name, largest first, each with the cores it used in the sample and its process count. Only names are printed, never a command line, environment or path.
pub(super) fn print(survey: &Survey) {
    let Some(host) = survey.hosts.iter().find(|host| host.name == survey.here) else { return };
    if !host.measure.is_some_and(|measure| measure.load > 2.0 * measure.cores) {
        return;
    }
    let rows = sample();
    if rows.is_empty() {
        println!("host {} cpu: not read: no cgroup CPU counters under {CGROUPS}", host.name);
    }
    for (name, cores, procs) in rows {
        println!("host {} cpu: {name} {cores:.2} cores, {procs} processes", host.name);
    }
}

fn sample() -> Vec<(String, f64, usize)> {
    let mut units = Vec::new();
    for slice in SLICES {
        leaves(&Path::new(CGROUPS).join(slice), DEPTH, &mut units);
    }
    let before: Vec<Option<u64>> = units.iter().map(|unit| usage_usec(unit)).collect();
    let started = Instant::now();
    std::thread::sleep(SAMPLE);
    let window_usec = started.elapsed().as_micros() as f64;
    let mut rows: Vec<(String, f64, usize)> = units
        .iter()
        .zip(before)
        .filter_map(|(unit, before)| {
            let used = usage_usec(unit)?.checked_sub(before?)?;
            Some((unit.file_name()?.to_string_lossy().into_owned(), used as f64 / window_usec, procs(unit)))
        })
        .collect();
    rows.sort_by(|a, b| b.1.total_cmp(&a.1));
    rows.truncate(TOP);
    rows
}

fn leaves(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let children: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().filter_map(|entry| entry.ok().map(|entry| entry.path())).filter(|path| path.is_dir()).collect();
    if children.is_empty() || depth == 0 {
        out.push(dir.to_path_buf());
    } else {
        for child in &children {
            leaves(child, depth - 1, out);
        }
    }
}

fn usage_usec(unit: &Path) -> Option<u64> {
    std::fs::read_to_string(unit.join("cpu.stat")).ok()?.lines().find_map(|line| line.strip_prefix("usage_usec ")?.parse().ok())
}

fn procs(unit: &Path) -> usize {
    std::fs::read_to_string(unit.join("cgroup.procs")).map_or(0, |text| text.lines().count())
}
