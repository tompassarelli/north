//! The coverage record beside a land queue's logs: one `landed` line per landed lane tip naming the
//! candidate log its pre-push selection wrote to, and one `judged` line per tip whose after-landing
//! failures `north test coverage miss` judged, with the count of failed tests the selection skipped.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const FILE: &str = "coverage-record.tsv";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Landed { sha: String, log: PathBuf },
    Judged { sha: String, misses: usize },
}

pub fn path(queue: &Path) -> PathBuf {
    queue.join("logs").join(FILE)
}

pub fn append(path: &Path, lines: &[Line]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text: String = lines
        .iter()
        .map(|line| match line {
            Line::Landed { sha, log } => format!("{sha}\tlanded\t{}\n", log.display()),
            Line::Judged { sha, misses } => format!("{sha}\tjudged\t{misses}\n"),
        })
        .collect();
    OpenOptions::new().create(true).append(true).open(path)?.write_all(text.as_bytes())
}

pub fn read(path: &Path) -> std::io::Result<Vec<Line>> {
    Ok(fs::read_to_string(path)?.lines().filter_map(parse).collect())
}

fn parse(line: &str) -> Option<Line> {
    let mut fields = line.splitn(3, '\t');
    let (sha, kind, detail) = (fields.next()?.to_owned(), fields.next()?, fields.next()?);
    match kind {
        "landed" => Some(Line::Landed { sha, log: detail.into() }),
        "judged" => Some(Line::Judged { sha, misses: detail.parse().ok()? }),
        _ => None,
    }
}
