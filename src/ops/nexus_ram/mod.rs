mod cpu;
mod report;
mod sample;
#[cfg(test)]
mod report_tests;
#[cfg(test)]
mod tests;

use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::foundation::clock::Timestamp;

const USAGE: &str = "usage: north nexus-ram record | report [--days N]";

pub(super) struct Inputs {
    dir: PathBuf,
    proc_root: PathBuf,
    zram: PathBuf,
    now: f64,
    journal: Option<PathBuf>,
}

impl Inputs {
    fn from_env() -> Self {
        let set = |name: &str| env::var(name).ok().filter(|value| !value.is_empty());
        let home = env::var("HOME").unwrap_or_default();
        Self {
            dir: set("NEXUS_RAM_DIR").map_or_else(|| Path::new(&home).join(".local/state/nexus-ram"), PathBuf::from),
            proc_root: PathBuf::from(set("NEXUS_RAM_PROC").unwrap_or_else(|| "/proc".to_owned())),
            zram: PathBuf::from(set("NEXUS_RAM_ZRAM").unwrap_or_else(|| "/sys/block/zram0/mm_stat".to_owned())),
            now: set("NEXUS_RAM_NOW").and_then(|value| value.parse().ok()).unwrap_or_else(|| Timestamp::now().unix_seconds_f64()),
            journal: set("NEXUS_RAM_JOURNAL").map(PathBuf::from),
        }
    }
}

pub(crate) fn run(arguments: &[String]) -> u8 {
    let inputs = Inputs::from_env();
    match arguments {
        [command] if command == "record" => match record(&inputs) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("nexus-ram: {}: {error}", inputs.dir.display());
                1
            }
        },
        [command, rest @ ..] if command == "report" => match days(rest) {
            Some(days) => report::report(&inputs, days, &mut io::stdout().lock()),
            None => usage(),
        },
        _ => usage(),
    }
}

fn usage() -> u8 {
    eprintln!("{USAGE}");
    2
}

fn days(rest: &[String]) -> Option<f64> {
    match rest {
        [] => Some(7.0),
        [flag, value] if flag == "--days" => value.parse().ok(),
        [flag] => flag.strip_prefix("--days=")?.parse().ok(),
        _ => None,
    }
}

fn day_file(dir: &Path, day: &str) -> PathBuf {
    dir.join(format!("{day}.jsonl"))
}

fn previous_row(dir: &Path, day: &str) -> Option<cpu::Previous> {
    let newest = format!("{day}.jsonl");
    let mut names = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.len() == newest.len() && name.ends_with(".jsonl") && *name <= newest)
        .collect::<Vec<_>>();
    names.sort_unstable();
    names.iter().rev().find_map(|name| sample::read(&dir.join(name)).lines().rev().find_map(|line| serde_json::from_str(line).ok()))
}

pub(super) fn record(inputs: &Inputs) -> io::Result<()> {
    let mut row = sample::sample(&inputs.proc_root, &inputs.zram, inputs.now);
    let day = row.ts[..10].to_owned();
    if let Some(previous) = previous_row(&inputs.dir, &day) {
        cpu::apply_previous(&mut row.cpu, &row.ts, &previous);
    }
    fs::create_dir_all(&inputs.dir)?;
    let line = serde_json::to_string(&row).map_err(io::Error::other)?;
    let mut out = OpenOptions::new().create(true).append(true).open(day_file(&inputs.dir, &day))?;
    writeln!(out, "{line}")
}
