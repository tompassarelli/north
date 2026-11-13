use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;

use super::Timings;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    Passed,
    Failed,
    Cancelled,
}

impl Outcome {
    pub fn of(status: i32) -> Self {
        if status == 0 { Outcome::Passed } else { Outcome::Failed }
    }

    fn name(self) -> &'static str {
        match self {
            Outcome::Passed => "passed",
            Outcome::Failed => "failed",
            Outcome::Cancelled => "cancelled",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "passed" => Some(Outcome::Passed),
            "failed" => Some(Outcome::Failed),
            "cancelled" => Some(Outcome::Cancelled),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CrateSplit {
    pub decided: u32,
    pub tested: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub started: u64,
    pub landing: String,
    pub paths: String,
    pub outcome: Outcome,
    pub exit: i32,
    pub wall: f64,
    pub cpu: Option<f64>,
    pub host: Option<String>,
    pub crates: Option<CrateSplit>,
    pub waited: Option<f64>,
    pub command: String,
}

impl Record {
    const OLD_FIELDS: usize = 8;
    const FIELDS: usize = 12;

    pub fn cost(&self) -> f64 {
        self.cpu.unwrap_or(self.wall)
    }

    fn line(&self) -> String {
        let clean = |text: &str| text.replace(['\t', '\n', ' '], "_");
        let text = |text: &str| text.replace(['\t', '\n'], " ");
        let number = |value: Option<f64>| value.map_or_else(|| "-".to_owned(), |value| format!("{value:.2}"));
        let host = self.host.as_deref().map_or_else(|| "-".to_owned(), clean);
        let (decided, tested) = self.crates.map_or_else(|| ("-".to_owned(), "-".to_owned()), |split| (split.decided.to_string(), split.tested.to_string()));
        format!("{}\t{}\t{}\t{}\t{}\t{:.2}\t{}\t{host}\t{decided}\t{tested}\t{}\t{}\n", self.started, text(&self.landing), text(&self.paths), self.outcome.name(), self.exit, self.wall, number(self.cpu), number(self.waited), text(&self.command))
    }

    fn parse(line: &str) -> Option<Self> {
        let fields: Vec<&str> = line.splitn(Self::FIELDS, '\t').collect();
        let (old, new) = (fields.len() == Self::OLD_FIELDS, fields.len() == Self::FIELDS);
        if !old && !new {
            return None;
        }
        let optional = |field: &str| match field {
            "-" => Some(None),
            field => field.parse().ok().map(Some),
        };
        let crates = if new { fields[8].parse().ok().zip(fields[9].parse().ok()).map(|(decided, tested)| CrateSplit { decided, tested }) } else { None };
        Some(Record {
            started: fields[0].parse().ok()?,
            landing: fields[1].to_owned(),
            paths: fields[2].to_owned(),
            outcome: Outcome::parse(fields[3])?,
            exit: fields[4].parse().ok()?,
            wall: fields[5].parse().ok()?,
            cpu: optional(fields[6])?,
            host: Some(fields[7]).filter(|host| new && *host != "-").map(str::to_owned),
            crates,
            waited: if new { optional(fields[10])? } else { None },
            command: fields[fields.len() - 1].to_owned(),
        })
    }
}

pub struct Records {
    path: PathBuf,
}

impl Records {
    pub fn at(path: PathBuf) -> Self {
        Records { path }
    }

    pub fn default_path() -> PathBuf {
        Timings::default_root().join("outcomes.tsv")
    }

    pub fn append(&self, record: &Record) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        OpenOptions::new().create(true).append(true).open(&self.path)?.write_all(record.line().as_bytes())
    }

    pub fn read(&self) -> Vec<Record> {
        fs::read_to_string(&self.path).unwrap_or_default().lines().filter_map(Record::parse).collect()
    }
}

#[cfg(test)]
#[path = "outcomes_tests.rs"]
mod tests;
