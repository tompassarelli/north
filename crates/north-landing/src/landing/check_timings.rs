pub mod outcomes;

use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::fmt;
use std::path::PathBuf;

pub const CEILING: u64 = 2100;
const FLOOR: u64 = 300;
const WINDOW: usize = 20;
const MEASURED: usize = 5;
const MARGIN: u64 = 3;
const EXCEEDED: &str = "exceeded";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Basis {
    Measured { p90: u64, passes: usize },
    Unmeasured { passes: usize },
    Raised { exceeded: u64 },
    Configured,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Deadline {
    pub seconds: u64,
    pub basis: Basis,
}

impl Deadline {
    pub fn configured(seconds: u64) -> Self {
        Deadline { seconds, basis: Basis::Configured }
    }

    pub fn from_passes(walls: &[u64]) -> Self {
        let recent = &walls[walls.len().saturating_sub(WINDOW)..];
        if recent.len() < MEASURED {
            return Deadline { seconds: CEILING, basis: Basis::Unmeasured { passes: recent.len() } };
        }
        let mut sorted = recent.to_vec();
        sorted.sort_unstable();
        let p90 = sorted[(sorted.len() * 9).div_ceil(10) - 1];
        Deadline { seconds: p90.saturating_mul(MARGIN).clamp(FLOOR, CEILING), basis: Basis::Measured { p90, passes: recent.len() } }
    }

    fn raised(self, exceeded: Option<u64>) -> Self {
        match exceeded.map(|exceeded| (exceeded, exceeded.saturating_mul(2).min(CEILING))) {
            Some((exceeded, seconds)) if seconds > self.seconds => Deadline { seconds, basis: Basis::Raised { exceeded } },
            _ => self,
        }
    }
}

impl fmt::Display for Deadline {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        let seconds = self.seconds;
        match self.basis {
            Basis::Measured { p90, passes } => write!(formatter, "deadline {seconds} s from p90 {p90} s over {passes} passes"),
            Basis::Unmeasured { passes: 1 } => write!(formatter, "deadline {seconds} s, 1 pass recorded"),
            Basis::Unmeasured { passes } => write!(formatter, "deadline {seconds} s, {passes} passes recorded"),
            Basis::Raised { exceeded } => write!(formatter, "deadline {seconds} s, raised from a {exceeded} s deadline that lane and base both ran past"),
            Basis::Configured => write!(formatter, "deadline {seconds} s from SAFE_PUSH_LAND_CHECK_TIMEOUT"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Timing {
    Finished(u64),
    Exceeded(u64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Base {
    Within(u64),
    Exceeded,
    Untimed,
}

pub struct Timings {
    root: PathBuf,
}

impl Timings {
    pub fn at(root: PathBuf) -> Self {
        Timings { root }
    }

    pub fn default_root() -> PathBuf {
        let state = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
        state.join("safe-push/check-timings")
    }

    fn key(command: &str, class: &str) -> String {
        let mut hasher = Sha256::new();
        for part in [command, class] {
            hasher.update(part.as_bytes());
            hasher.update([0]);
        }
        hasher.finalize().iter().take(16).map(|byte| format!("{byte:02x}")).collect()
    }

    fn file(&self, tree: &str, command: &str, class: &str) -> io::Result<PathBuf> {
        self.entry(tree, command, class, "tsv")
    }

    fn entry(&self, tree: &str, command: &str, class: &str, extension: &str) -> io::Result<PathBuf> {
        if tree.is_empty() || !tree.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "a check timing names a tree by its hex object id"));
        }
        Ok(self.root.join(tree).join(format!("{}.{extension}", Self::key(command, class))))
    }

    fn passes_file(&self, command: &str, class: &str) -> PathBuf {
        self.root.join("passes").join(format!("{}.tsv", Self::key(command, class)))
    }

    pub fn passed(&self, command: &str, class: &str, seconds: u64) -> io::Result<()> {
        let path = self.passes_file(command, class);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        OpenOptions::new().create(true).append(true).open(path)?.write_all(format!("{seconds}\n").as_bytes())
    }

    pub fn exceeded_on_base_too(&self, command: &str, class: &str, deadline: u64) -> io::Result<()> {
        let path = self.passes_file(command, class);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        OpenOptions::new().create(true).append(true).open(path)?.write_all(format!("{EXCEEDED}\t{deadline}\n").as_bytes())
    }

    pub fn deadline(&self, command: &str, class: &str) -> Deadline {
        let text = fs::read_to_string(self.passes_file(command, class)).unwrap_or_default();
        let lines: Vec<&str> = text.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
        let recent = &lines[lines.len().saturating_sub(WINDOW)..];
        let walls: Vec<u64> = recent.iter().filter_map(|line| line.parse().ok()).collect();
        let exceeded = recent.iter().filter_map(|line| line.strip_prefix(EXCEEDED)?.trim().parse().ok()).max();
        Deadline::from_passes(&walls).raised(exceeded)
    }

    pub fn record(&self, tree: &str, command: &str, class: &str, timing: Timing) -> io::Result<()> {
        let path = self.file(tree, command, class)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let line = match timing {
            Timing::Finished(seconds) => format!("finished\t{seconds}\n"),
            Timing::Exceeded(seconds) => format!("exceeded\t{seconds}\n"),
        };
        OpenOptions::new().create(true).append(true).open(path)?.write_all(line.as_bytes())
    }

    pub fn base(&self, tree: &str, command: &str, class: &str, deadline: u64) -> Base {
        let text = self.file(tree, command, class).and_then(fs::read_to_string).unwrap_or_default();
        let timings: Vec<Timing> = text
            .lines()
            .filter_map(|line| {
                let (kind, seconds) = line.split_once('\t')?;
                let seconds = seconds.trim().parse().ok()?;
                match kind {
                    "finished" => Some(Timing::Finished(seconds)),
                    "exceeded" => Some(Timing::Exceeded(seconds)),
                    _ => None,
                }
            })
            .collect();
        if timings.is_empty() {
            return Base::Untimed;
        }
        let mut within = None;
        for timing in timings {
            match timing {
                Timing::Finished(seconds) if seconds <= deadline => within = within.max(Some(seconds)),
                _ => return Base::Exceeded,
            }
        }
        within.map_or(Base::Exceeded, Base::Within)
    }

    pub fn record_result(&self, tree: &str, command: &str, class: &str, round: &str, result: &BaseResult) -> io::Result<()> {
        let path = self.entry(tree, command, class, "result")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let line = match result {
            BaseResult::Passed => format!("{round}\tpassed\t\n"),
            BaseResult::Failed(first) => format!("{round}\tfailed\t{}\n", first.replace(['\t', '\n'], " ")),
        };
        fs::write(path, line)
    }

    pub fn record_findings(&self, tree: &str, command: &str, class: &str, round: &str, findings: &str) -> io::Result<()> {
        let path = self.entry(tree, command, class, "findings")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, format!("{round}\n{findings}"))
    }

    pub fn findings(&self, tree: &str, command: &str, class: &str, round: &str) -> Option<String> {
        let text = self.entry(tree, command, class, "findings").and_then(fs::read_to_string).ok()?;
        let (recorded, findings) = text.split_once('\n')?;
        (!round.is_empty() && recorded == round).then(|| findings.to_owned())
    }

    pub fn result(&self, tree: &str, command: &str, class: &str, round: &str) -> Option<BaseResult> {
        let text = self.entry(tree, command, class, "result").and_then(fs::read_to_string).ok()?;
        let mut fields = text.trim_end_matches('\n').splitn(3, '\t');
        if round.is_empty() || fields.next()? != round {
            return None;
        }
        match (fields.next()?, fields.next().unwrap_or_default()) {
            ("passed", _) => Some(BaseResult::Passed),
            ("failed", first) => Some(BaseResult::Failed(first.to_owned())),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BaseResult {
    Passed,
    Failed(String),
}
