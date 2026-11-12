use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const MARK: &str = "north-reuse";
const DIRECTORY: &str = "north/land-reuse";
const DAY: u64 = 86_400;
const KEPT_DAYS: u64 = 14;

#[derive(Clone, Debug, PartialEq)]
pub enum Record {
    Unit { probe: String, unit: String, set: Option<String>, passed: bool, seconds: f64 },
    Recorded { probe: String, unit: String, set: String },
    SetDiffers { probe: String, unit: String, reused: String, real: String, path: String },
}

impl Record {
    pub fn render(&self) -> String {
        match self {
            Self::Unit { probe, unit, set, passed, seconds } => format!("unit\t{probe}\t{unit}\t{}\t{}\t{seconds:.3}", set.as_deref().unwrap_or("-"), if *passed { "pass" } else { "fail" }),
            Self::Recorded { probe, unit, set } => format!("recorded\t{probe}\t{unit}\t{set}"),
            Self::SetDiffers { probe, unit, reused, real, path } => format!("set-differs\t{probe}\t{unit}\t{reused}\t{real}\t{path}"),
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let fields: Vec<&str> = text.split('\t').collect();
        match fields.as_slice() {
            ["unit", probe, unit, set, verdict, seconds] => Some(Self::Unit { probe: (*probe).to_owned(), unit: (*unit).to_owned(), set: (*set != "-").then(|| (*set).to_owned()), passed: *verdict == "pass", seconds: seconds.parse().ok()? }),
            ["recorded", probe, unit, set] => Some(Self::Recorded { probe: (*probe).to_owned(), unit: (*unit).to_owned(), set: (*set).to_owned() }),
            ["set-differs", probe, unit, reused, real, path] => Some(Self::SetDiffers { probe: (*probe).to_owned(), unit: (*unit).to_owned(), reused: (*reused).to_owned(), real: (*real).to_owned(), path: (*path).to_owned() }),
            _ => None,
        }
    }

    pub fn marked(&self) -> String {
        format!("{MARK}\t{}", self.render())
    }

    pub fn from_marked(line: &str) -> Option<Self> {
        Self::parse(line.strip_prefix(MARK)?.strip_prefix('\t')?)
    }
}

pub fn directory(state_home: &Path) -> PathBuf {
    state_home.join(DIRECTORY)
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
    let records: Vec<Record> = shadow.iter().filter_map(|line| Record::from_marked(line)).collect();
    append(&records, directory);
    let _ = fs::write(log, kept.iter().map(|line| format!("{line}\n")).collect::<String>());
}

pub fn append(records: &[Record], directory: &Path) {
    if records.is_empty() {
        return;
    }
    let epoch = now();
    let block: String = records.iter().map(|record| format!("{epoch}\t{}\n", record.render())).collect();
    if fs::create_dir_all(directory).is_ok() && let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(day_file(directory, epoch)) {
        let _ = file.write_all(block.as_bytes());
    }
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
