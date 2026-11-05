use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::issue_box_lint;
use crate::land::after::tick as landing;
use crate::land::queue;
use crate::platform::process;

pub const DIRECTORY: &str = "issue-bodies";
pub const REFUSED: &str = "refused";
pub const CALL_LIMIT: Duration = Duration::from_secs(30);
pub const DRAIN_LIMIT: Duration = Duration::from_secs(120);
const ATTEMPTS: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub repo: String,
    pub number: u64,
}

impl Issue {
    pub fn parse(item: &str) -> Result<Issue, String> {
        let (repo, number) = issue_box_lint::issue(item)?;
        let number = number.parse().map_err(|_| format!("{item}: the issue number is too large"))?;
        Ok(Issue { repo: issue_box_lint::slug(repo), number })
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}#{}", self.repo, self.number)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Edit {
    Append { text: String },
    Tick { box_text: String },
    Land { box_text: String, sha: String },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Change {
    Applied(String),
    Already,
    Refused(String),
}

fn without_checkbox(line: &str) -> &str {
    let line = line.trim();
    for marker in ["- [ ]", "- [x]", "- [X]", "* [ ]", "* [x]", "* [X]"] {
        if let Some(rest) = line.strip_prefix(marker) {
            return rest.trim();
        }
    }
    line
}

fn ticked(line: &str) -> Option<&str> {
    let line = line.trim_start();
    ["- [x]", "- [X]", "* [x]", "* [X]"].iter().find_map(|marker| line.strip_prefix(marker)).map(str::trim)
}

fn unticked(line: &str) -> Option<&str> {
    let line = line.trim_start();
    line.strip_prefix("- [ ]").or_else(|| line.strip_prefix("* [ ]")).map(str::trim)
}

impl Edit {
    pub fn apply(&self, body: &str) -> Change {
        match self {
            Edit::Append { text } => append(body, text),
            Edit::Tick { box_text } => tick(body, box_text),
            Edit::Land { box_text, sha } => match landing::tick(body, box_text, Some(sha)) {
                landing::Tick::Ticked(edited) => Change::Applied(edited),
                landing::Tick::AlreadyTicked => Change::Already,
                refused => Change::Refused(refused.to_string()),
            },
        }
    }
}

fn append(body: &str, text: &str) -> Change {
    let wanted: Vec<&str> = text.lines().map(without_checkbox).filter(|line| !line.is_empty()).collect();
    if wanted.is_empty() {
        return Change::Refused("the text to append is empty".to_owned());
    }
    let present: Vec<&str> = body.lines().map(without_checkbox).collect();
    if wanted.iter().all(|line| present.contains(line)) {
        return Change::Already;
    }
    let kept = body.trim_end();
    let text = text.trim_end();
    Change::Applied(if kept.is_empty() { format!("{text}\n") } else { format!("{kept}\n{text}\n") })
}

fn tick(body: &str, box_text: &str) -> Change {
    let wanted = box_text.trim();
    if wanted.is_empty() {
        return Change::Refused("the box text is empty".to_owned());
    }
    let lines: Vec<&str> = body.split_inclusive('\n').collect();
    let open: Vec<usize> = lines.iter().enumerate().filter(|(_, line)| unticked(line).is_some_and(|text| text.contains(wanted))).map(|(index, _)| index).collect();
    match open.as_slice() {
        [index] => {
            let mut edited = String::with_capacity(body.len());
            for (position, line) in lines.iter().enumerate() {
                if position == *index {
                    edited.push_str(&line.replacen("[ ]", "[x]", 1));
                } else {
                    edited.push_str(line);
                }
            }
            Change::Applied(edited)
        }
        [] if lines.iter().any(|line| ticked(line).is_some_and(|text| text.contains(wanted))) => Change::Already,
        [] => Change::Refused(format!("no unticked box contains {wanted:?}")),
        _ => Change::Refused(format!("{} unticked boxes contain {wanted:?}", open.len())),
    }
}

pub trait Github {
    fn body(&self, issue: &Issue) -> Result<String, String>;
    fn set_body(&self, issue: &Issue, body: &str) -> Result<(), String>;
    fn close(&self, issue: &Issue, comment: &str) -> Result<(), String>;
}

pub struct Gh {
    program: PathBuf,
    limit: Duration,
}

impl Gh {
    pub fn new(limit: Duration) -> Gh {
        Gh { program: PathBuf::from("gh"), limit }
    }

    pub fn at(program: impl Into<PathBuf>, limit: Duration) -> Gh {
        Gh { program: program.into(), limit }
    }

    fn run(&self, arguments: &[&str], input: Option<&[u8]>) -> Result<Vec<u8>, String> {
        let verb = arguments[..2].join(" ");
        let output = process::run_with_input_within(&self.program.to_string_lossy(), arguments, input, self.limit)
            .ok_or_else(|| format!("gh {verb} timed out after {} s", self.limit.as_secs_f64()))?;
        if output.status.success() {
            return Ok(output.stdout);
        }
        Err(format!("gh {verb}: {}", String::from_utf8_lossy(&output.stderr).trim()))
    }
}

impl Github for Gh {
    fn body(&self, issue: &Issue) -> Result<String, String> {
        let number = issue.number.to_string();
        let stdout = self.run(&["issue", "view", &number, "--repo", &issue.repo, "--json", "body", "--jq", ".body"], None)?;
        let text = String::from_utf8(stdout).map_err(|error| error.to_string())?;
        Ok(text.strip_suffix('\n').unwrap_or(&text).to_owned())
    }

    fn set_body(&self, issue: &Issue, body: &str) -> Result<(), String> {
        let number = issue.number.to_string();
        self.run(&["issue", "edit", &number, "--repo", &issue.repo, "--body-file", "-"], Some(body.as_bytes())).map(|_| ())
    }

    fn close(&self, issue: &Issue, comment: &str) -> Result<(), String> {
        let number = issue.number.to_string();
        self.run(&["issue", "close", &number, "--repo", &issue.repo, "--comment", comment], None).map(|_| ())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Written,
    Already,
    Refused(String),
}

pub fn write(github: &dyn Github, issue: &Issue, edit: &Edit, deadline: Instant) -> Result<Outcome, String> {
    for _ in 0..ATTEMPTS {
        if Instant::now() >= deadline {
            return Err(format!("{issue}: the deadline passed before the edit was verified"));
        }
        let next = match edit.apply(&github.body(issue)?) {
            Change::Already => return Ok(Outcome::Already),
            Change::Refused(why) => return Ok(Outcome::Refused(why)),
            Change::Applied(next) => next,
        };
        github.set_body(issue, &next)?;
        if edit.apply(&github.body(issue)?) == Change::Already {
            return Ok(Outcome::Written);
        }
    }
    Err(format!("{issue}: the edit did not stay after {ATTEMPTS} attempts (a concurrent writer replaced the body, or a write was dropped)"))
}

pub fn close_when_done(github: &dyn Github, issue: &Issue, edit: &Edit) -> Result<bool, String> {
    let Edit::Land { sha, .. } = edit else { return Ok(false) };
    if !landing::closable(&github.body(issue)?) {
        return Ok(false);
    }
    github.close(issue, &format!("Closed by the landing of {sha}: every box on this issue is ticked."))?;
    Ok(true)
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub issue: Issue,
    pub edit: Edit,
}

pub fn directory(queue_directory: &Path) -> PathBuf {
    queue_directory.join(DIRECTORY)
}

pub fn submit(queue_directory: &Path, issue: &Issue, edit: &Edit) -> io::Result<PathBuf> {
    let directory = directory(queue_directory);
    let text = serde_json::to_string_pretty(&Entry { issue: issue.clone(), edit: edit.clone() }).map_err(io::Error::other)? + "\n";
    let digest: String = Sha256::digest(text.as_bytes()).iter().take(6).map(|byte| format!("{byte:02x}")).collect();
    let suffix = format!("-{digest}.json");
    if let Some((path, _)) = pending(&directory)?.into_iter().find(|(path, _)| path.to_string_lossy().ends_with(&suffix)) {
        return Ok(path);
    }
    let micros = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_micros());
    let path = directory.join(format!("{micros}{suffix}"));
    queue::write_atomic(&path, &text)?;
    Ok(path)
}

pub fn pending(directory: &Path) -> io::Result<Vec<(PathBuf, Entry)>> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut found = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "json") && path.is_file() {
            match fs::read_to_string(&path).map_err(|error| error.to_string()).and_then(|text| serde_json::from_str::<Entry>(&text).map_err(|error| error.to_string())) {
                Ok(entry) => found.push((path, entry)),
                Err(why) => refuse(&path, &format!("unreadable entry: {why}"))?,
            }
        }
    }
    found.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(found)
}

fn refuse(path: &Path, why: &str) -> io::Result<()> {
    let directory = path.parent().ok_or_else(|| io::Error::other(format!("{} has no parent", path.display())))?.join(REFUSED);
    fs::create_dir_all(&directory)?;
    let name = path.file_name().ok_or_else(|| io::Error::other(format!("{} has no file name", path.display())))?;
    fs::write(directory.join(Path::new(name).with_extension("why")), format!("{why}\n"))?;
    fs::rename(path, directory.join(name))
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Drained {
    pub written: usize,
    pub already: usize,
    pub refused: usize,
    pub closed: usize,
    pub waiting: usize,
    pub failure: Option<String>,
}

pub fn drain(directory: &Path, github: &dyn Github, limit: Duration) -> io::Result<Drained> {
    let deadline = Instant::now() + limit;
    let entries = pending(directory)?;
    let mut drained = Drained::default();
    for (position, (path, entry)) in entries.iter().enumerate() {
        if Instant::now() >= deadline {
            drained.waiting = entries.len() - position;
            break;
        }
        match write(github, &entry.issue, &entry.edit, deadline) {
            Ok(outcome @ (Outcome::Written | Outcome::Already)) => match close_when_done(github, &entry.issue, &entry.edit) {
                Ok(closed) => {
                    if outcome == Outcome::Written {
                        drained.written += 1;
                    } else {
                        drained.already += 1;
                    }
                    drained.closed += usize::from(closed);
                    fs::remove_file(path)?;
                }
                Err(why) => {
                    drained.waiting = entries.len() - position;
                    drained.failure = Some(format!("{}: {why}", entry.issue));
                    break;
                }
            },
            Ok(Outcome::Refused(why)) => {
                drained.refused += 1;
                refuse(path, &format!("{}: {why}", entry.issue))?;
            }
            Err(why) => {
                drained.waiting = entries.len() - position;
                drained.failure = Some(why);
                break;
            }
        }
    }
    Ok(drained)
}

pub fn drain_in_background(directory: PathBuf, github: impl Github + Send + 'static, limit: Duration) -> JoinHandle<io::Result<Drained>> {
    std::thread::spawn(move || drain(&directory, &github, limit))
}

#[cfg(test)]
#[path = "issue_body_tests.rs"]
mod tests;
