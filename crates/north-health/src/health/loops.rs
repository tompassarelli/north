use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use north_base::cites;

const UNREAD_SECONDS: u64 = 7 * 24 * 60 * 60;
const UNVERIFIABLE: &str = "unverifiable";

pub(super) struct Report {
    pub stale: Vec<String>,
    pub unverifiable: Vec<String>,
}

pub(crate) fn section<'a>(doc: &'a str, heading: &str) -> Vec<Vec<&'a str>> {
    let mut inside = false;
    let mut rows = Vec::new();
    for line in doc.lines() {
        if line.starts_with('#') {
            inside = line.trim_start_matches('#').trim() == heading;
            continue;
        }
        if inside && line.starts_with('|') {
            let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
            if !cells.iter().all(|cell| cell.chars().all(|c| c == '-' || c == ':')) {
                rows.push(cells);
            }
        }
    }
    rows.into_iter().skip(1).collect()
}

struct WorkingCopy<'a>(&'a Path);

impl cites::Tree for WorkingCopy<'_> {
    fn exists(&self, path: &str) -> bool {
        self.0.join(path).exists()
    }

    fn text(&self, path: &str) -> Option<String> {
        source_text(&self.0.join(path))
    }
}

fn source_text(target: &Path) -> Option<String> {
    if target.is_file() {
        return fs::read_to_string(target).ok();
    }
    let mut text = String::new();
    for entry in fs::read_dir(target).ok()?.flatten() {
        if let Some(child) = source_text(&entry.path()) {
            text.push_str(&child);
        }
    }
    Some(text)
}

pub(super) fn unread_signals(doc: &str, modified: &dyn Fn(&str) -> Option<u64>) -> Report {
    let mut report = Report { stale: Vec::new(), unverifiable: Vec::new() };
    for cells in section(doc, "File signals") {
        let [file, stamp, ..] = cells[..] else { continue };
        let file = file.trim_matches('`');
        let stamp = stamp.trim_matches('`');
        if stamp == UNVERIFIABLE {
            report.unverifiable.push(file.to_owned());
            continue;
        }
        let Some(written) = modified(file) else { continue };
        let read = modified(stamp).unwrap_or(0);
        if written > read + UNREAD_SECONDS {
            report.stale.push(format!("{file}: written {} days after its reader's {stamp}", (written - read) / 86_400));
        }
    }
    report
}

pub(super) fn state_modified(base: PathBuf) -> impl Fn(&str) -> Option<u64> {
    move |relative| {
        let path = base.join(relative);
        let stamp = fs::metadata(&path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
        Some(stamp)
    }
}

pub(super) fn verdict(root: &Path, base: PathBuf) -> (Result<(), String>, Vec<String>) {
    let Ok(doc) = fs::read_to_string(root.join(cites::FEEDBACK_LOOP)) else {
        return (Err(format!("{} missing", cites::FEEDBACK_LOOP)), Vec::new());
    };
    let tree = WorkingCopy(root);
    let mut faults = cites::dead_readers(&doc, &tree);
    faults.extend(cites::dead_loop_sites(&fs::read_to_string(root.join(cites::LOOPS)).unwrap_or_default(), &tree));
    let report = unread_signals(&doc, &state_modified(base));
    faults.extend(report.stale);
    let result = if faults.is_empty() { Ok(()) } else { Err(format!("{} dead feedback-loop cites: {}", faults.len(), faults.join("; "))) };
    (result, report.unverifiable)
}

pub enum Origin {
    File,
    Query { database: String, query: String },
}

pub struct Declared {
    pub name: String,
    pub origin: Origin,
    pub writer: String,
    pub cadence_s: f64,
    pub owner: Option<String>,
}

pub(crate) fn cadence_seconds(text: &str) -> Option<f64> {
    let text = text.trim().trim_matches('`');
    let (number, unit) = text.split_at(text.find(|c: char| !c.is_ascii_digit())?);
    let scale = match unit {
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        "d" => 86_400.0,
        _ => return None,
    };
    number.parse::<f64>().ok().filter(|count| *count > 0.0).map(|count| count * scale)
}

fn owner_of(cell: Option<&&str>) -> Option<String> {
    cell.map(|cell| cell.trim_matches('`')).filter(|cell| cell.contains('#')).map(str::to_owned)
}

pub fn declared_files(doc: &str) -> Vec<Declared> {
    section(doc, "File signals")
        .into_iter()
        .filter_map(|cells| Some(Declared { name: cells[0].trim_matches('`').to_owned(), origin: Origin::File, writer: (*cells.get(2)?).to_owned(), cadence_s: cadence_seconds(cells.get(3)?)?, owner: owner_of(cells.get(4)) }))
        .collect()
}

pub fn declared_readings(doc: &str) -> Vec<Declared> {
    section(doc, "Recorded readings")
        .into_iter()
        .filter_map(|cells| {
            let origin = Origin::Query { database: cells.get(1)?.trim_matches('`').to_owned(), query: cells.get(2)?.trim_matches('`').to_owned() };
            Some(Declared { name: cells[0].to_owned(), origin, writer: (*cells.get(3)?).to_owned(), cadence_s: cadence_seconds(cells.get(4)?)?, owner: owner_of(cells.get(5)) })
        })
        .collect()
}
