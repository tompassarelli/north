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

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub started: u64,
    pub landing: String,
    pub paths: String,
    pub outcome: Outcome,
    pub exit: i32,
    pub wall: f64,
    pub cpu: Option<f64>,
    pub command: String,
}

impl Record {
    pub fn cost(&self) -> f64 {
        self.cpu.unwrap_or(self.wall)
    }

    fn line(&self) -> String {
        let cpu = self.cpu.map_or_else(|| "-".to_owned(), |cpu| format!("{cpu:.2}"));
        let clean = |text: &str| text.replace(['\t', '\n'], " ");
        format!("{}\t{}\t{}\t{}\t{}\t{:.2}\t{cpu}\t{}\n", self.started, clean(&self.landing), clean(&self.paths), self.outcome.name(), self.exit, self.wall, clean(&self.command))
    }

    fn parse(line: &str) -> Option<Self> {
        let mut fields = line.splitn(8, '\t');
        let started = fields.next()?.parse().ok()?;
        let landing = fields.next()?.to_owned();
        let paths = fields.next()?.to_owned();
        let outcome = Outcome::parse(fields.next()?)?;
        let exit = fields.next()?.parse().ok()?;
        let wall = fields.next()?.parse().ok()?;
        let cpu = match fields.next()? {
            "-" => None,
            cpu => Some(cpu.parse().ok()?),
        };
        let command = fields.next()?.to_owned();
        Some(Record { started, landing, paths, outcome, exit, wall, cpu, command })
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
