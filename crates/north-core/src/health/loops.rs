use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const ROOTS: &[&str] = &["bin/", "crates/", "harness/", "src/", "docs/", "agent-machinery/", "lib/"];
const UNREAD_SECONDS: u64 = 7 * 24 * 60 * 60;
const UNVERIFIABLE: &str = "unverifiable";

pub(super) struct Report {
    pub stale: Vec<String>,
    pub unverifiable: Vec<String>,
}

fn section<'a>(doc: &'a str, heading: &str) -> Vec<Vec<&'a str>> {
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

fn signal_rows(doc: &str) -> Vec<Vec<&str>> {
    let mut rows = Vec::new();
    let mut inside = false;
    for line in doc.lines() {
        if line.starts_with("| signal | writer | reader on main") {
            inside = true;
            continue;
        }
        if inside && !line.starts_with('|') {
            break;
        }
        if inside && !line.starts_with("|---") {
            rows.push(line.trim().trim_matches('|').split('|').map(str::trim).collect());
        }
    }
    rows
}

enum Cite {
    Path(String, Option<usize>),
    Line(usize),
    Symbol(String),
}

fn classify(raw: &str) -> Option<Cite> {
    let token = raw.trim_matches(|c: char| matches!(c, ',' | ';' | '.' | '`' | '(' | ')'));
    if let Some(line) = token.strip_prefix(':') {
        return line.parse().ok().map(Cite::Line);
    }
    if ROOTS.iter().any(|root| token.starts_with(root)) {
        return Some(match token.rsplit_once(':') {
            Some((path, line)) if line.parse::<usize>().is_ok() => Cite::Path(path.into(), line.parse().ok()),
            _ => Cite::Path(token.into(), None),
        });
    }
    let word = !token.is_empty() && token.contains('_') && token.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    word.then(|| Cite::Symbol(token.into()))
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

fn resolve(root: &Path, path: &str, line: Option<usize>, symbol: Option<&str>) -> Result<(), String> {
    let target = root.join(path);
    if !target.exists() {
        return Err(format!("{path} does not exist"));
    }
    if let Some(line) = line {
        let length = fs::read_to_string(&target).map(|text| text.lines().count()).unwrap_or(0);
        if line > length {
            return Err(format!("{path}:{line} is past the end ({length} lines)"));
        }
    }
    if let Some(symbol) = symbol
        && !source_text(&target).is_some_and(|text| text.contains(symbol))
    {
        return Err(format!("{symbol} is not in {path}"));
    }
    Ok(())
}

fn check_cell(root: &Path, cell: &str) -> Vec<String> {
    let mut faults = Vec::new();
    let mut current: Option<(String, Option<usize>)> = None;
    let mut fresh = false;
    for word in cell.split_whitespace() {
        match classify(word) {
            Some(Cite::Path(path, line)) => {
                if let Err(fault) = resolve(root, &path, line, None) {
                    faults.push(fault);
                }
                current = Some((path, line));
                fresh = true;
            }
            Some(Cite::Line(line)) => {
                if let Some((path, _)) = &current {
                    if let Err(fault) = resolve(root, path, Some(line), None) {
                        faults.push(fault);
                    }
                    fresh = true;
                }
            }
            Some(Cite::Symbol(symbol)) => {
                if let (true, Some((path, _))) = (fresh, &current)
                    && let Err(fault) = resolve(root, path, None, Some(&symbol))
                {
                    faults.push(fault);
                }
                fresh = false;
            }
            None => fresh = false,
        }
    }
    faults.dedup();
    faults
}

pub(super) fn stale_readers(root: &Path, doc: &str) -> Vec<String> {
    signal_rows(doc)
        .into_iter()
        .filter(|cells| cells.len() >= 3)
        .flat_map(|cells| {
            let name = cells[0].to_owned();
            check_cell(root, cells[2]).into_iter().map(move |fault| format!("{name}: {fault}"))
        })
        .collect()
}

pub(super) fn unread_signals(doc: &str, modified: &dyn Fn(&str) -> Option<u64>) -> Report {
    let mut report = Report { stale: Vec::new(), unverifiable: Vec::new() };
    for cells in section(doc, "File signals") {
        let [file, stamp] = cells[..] else { continue };
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
    let Ok(doc) = fs::read_to_string(root.join("docs/feedback-loop.md")) else {
        return (Err("docs/feedback-loop.md missing".into()), Vec::new());
    };
    let mut faults = stale_readers(root, &doc);
    let report = unread_signals(&doc, &state_modified(base));
    faults.extend(report.stale);
    let result = if faults.is_empty() { Ok(()) } else { Err(format!("{} dead feedback-loop cites: {}", faults.len(), faults.join("; "))) };
    (result, report.unverifiable)
}
