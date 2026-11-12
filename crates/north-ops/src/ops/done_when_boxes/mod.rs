use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use tempfile::TempDir;

use crate::error::{NorthError, NorthResult};
use crate::foundation::check_lane;
use crate::foundation::clock::Timestamp;
use crate::ledger::{CheckRun, Ledger};
use crate::platform::rolling_issue::{Gh, Tracker};
use crate::platform::{git, github, process};

pub use evidence::ticked;
pub use expectation::{Expectation, Expected, Outcome};

mod evidence;
mod expectation;

const MARKER: &str = "Done when: `";
const OPEN: &str = "- [ ] ";
const LIMIT: Duration = Duration::from_secs(600);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Check {
    pub text: String,
    pub command: String,
    pub expected: Expected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Run {
    pub exit: i32,
    pub stdout: String,
    pub output: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Verdict {
    pub check: Check,
    pub run: Run,
    pub outcome: Outcome,
}

impl Verdict {
    pub fn passed(&self) -> bool {
        self.outcome == Outcome::Pass
    }

    pub fn line(&self, label: &str) -> String {
        let reason = match &self.check.expected {
            Expected::Judge(reason) => format!(" [{reason}]"),
            Expected::All(_) => String::new(),
        };
        format!("{label}: {} (exit {}) `{}`: {}{reason}", self.outcome.label(), self.run.exit, self.check.command, self.check.text)
    }
}

pub struct Stamp {
    pub sha: String,
    pub date: String,
}

pub fn checks(body: &str) -> Vec<Check> {
    body.lines()
        .filter_map(|line| {
            let rest = line.trim_start().strip_prefix(OPEN)?;
            let after = &rest[rest.find(MARKER)? + MARKER.len()..];
            let close = after.find('`')?;
            let command = after[..close].trim();
            (!command.is_empty()).then(|| Check { text: rest.trim().to_owned(), command: command.to_owned(), expected: expectation::expected(&after[close + 1..]) })
        })
        .collect()
}

pub fn judge(command: &str, main: &Path) -> Run {
    judge_under(plain(command), main)
}

fn plain(command: &str) -> Command {
    let mut shell = Command::new("sh");
    shell.args(["-c", command]);
    shell
}

fn judge_under(mut shell: Command, main: &Path) -> Run {
    shell.current_dir(main);
    match process::run_group_within(shell, LIMIT) {
        Some(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let output_text = format!("{stdout}{}", String::from_utf8_lossy(&output.stderr));
            Run { exit: output.status.code().unwrap_or(1), stdout, output: output_text }
        }
        None => Run { exit: 124, stdout: String::new(), output: format!("no exit within {}s", LIMIT.as_secs()) },
    }
}

pub fn verdicts(body: &str, run: &dyn Fn(&str) -> Run) -> Vec<Verdict> {
    checks(body)
        .into_iter()
        .map(|check| {
            let run = run(&check.command);
            let outcome = expectation::outcome(&check.expected, run.exit, &run.stdout, &run.output);
            Verdict { check, run, outcome }
        })
        .collect()
}

pub fn settle(tracker: &mut dyn Tracker, number: u64, verdicts: &[Verdict], stamp: &Stamp) -> Result<(), String> {
    if !verdicts.iter().any(Verdict::passed) {
        return Ok(());
    }
    let fresh = tracker.body(number)?;
    let edited = ticked(&fresh, verdicts, stamp);
    if edited != fresh {
        tracker.edit(number, &edited)?;
    }
    Ok(())
}

fn record(ledger: &Ledger, task_ref: &str, verdicts: &[Verdict], main: &Path, stamp: &Stamp) -> NorthResult<()> {
    for verdict in verdicts {
        let exit = if verdict.passed() { 0 } else { verdict.run.exit.max(1) };
        ledger.record_check(&CheckRun { task_ref, command: &verdict.check.command, cwd: main, head_sha: Some(&stamp.sha), exit, ran_ms: Timestamp::now().unix_millis() })?;
    }
    Ok(())
}

pub enum Main {
    Given(PathBuf),
    Fetched { repo: PathBuf, checkout: Checkout },
}

pub enum Checkout {
    Lane { path: PathBuf, _lock: File },
    Scratch(TempDir),
}

impl Main {
    pub fn fetch(repo: &Path) -> NorthResult<Self> {
        git::stdout(repo, &["fetch", "--quiet", "origin", "main"], |output| failed("fetch origin main", output)).map_err(NorthError::Refused)?;
        let checkout = match container(repo) {
            Some((container, common)) => Self::lane(repo, &container, &common)?,
            None => Self::scratch(repo)?,
        };
        Ok(Self::Fetched { repo: repo.to_owned(), checkout })
    }

    fn lane(repo: &Path, container: &Path, common: &Path) -> NorthResult<Checkout> {
        let path = check_lane::of(container);
        std::fs::create_dir_all(&path)?;
        let lock = std::fs::OpenOptions::new().create(true).write(true).truncate(false).open(common.join("north-done-when.lock"))?;
        lock.lock()?;
        if path.join(".git").exists() {
            git::stdout(&path, &["checkout", "--quiet", "--detach", "--force", "refs/remotes/origin/main"], |output| failed("checkout origin/main", output)).map_err(NorthError::Refused)?;
            git::stdout(&path, &["clean", "-fdq"], |output| failed("clean", output)).map_err(NorthError::Refused)?;
        } else {
            let location = path.to_string_lossy().into_owned();
            git::stdout(repo, &["worktree", "add", "--quiet", "--detach", "--force", &location, "refs/remotes/origin/main"], |output| failed("worktree add", output)).map_err(NorthError::Refused)?;
        }
        Ok(Checkout::Lane { path, _lock: lock })
    }

    fn scratch(repo: &Path) -> NorthResult<Checkout> {
        let checkout = tempfile::Builder::new().prefix("done-when-").tempdir()?;
        let path = checkout.path().to_string_lossy().into_owned();
        git::stdout(repo, &["worktree", "add", "--quiet", "--detach", &path, "refs/remotes/origin/main"], |output| failed("worktree add", output)).map_err(NorthError::Refused)?;
        Ok(Checkout::Scratch(checkout))
    }

    pub fn dir(&self) -> &Path {
        match self {
            Self::Given(dir) => dir,
            Self::Fetched { checkout: Checkout::Lane { path, .. }, .. } => path,
            Self::Fetched { checkout: Checkout::Scratch(checkout), .. } => checkout.path(),
        }
    }

    pub fn sha(&self) -> NorthResult<String> {
        let sha = git::stdout(self.dir(), &["rev-parse", "HEAD"], |output| format!("git rev-parse HEAD: {}", String::from_utf8_lossy(&output.stderr).trim()))
            .map_err(NorthError::Refused)?;
        Ok(String::from_utf8_lossy(&sha).trim().to_owned())
    }
}

impl Drop for Main {
    fn drop(&mut self) {
        if let Self::Fetched { repo, checkout: Checkout::Scratch(checkout) } = self {
            let _ = git::run(repo, &["worktree", "remove", "--force", &checkout.path().to_string_lossy()]);
        }
    }
}

fn failed(what: &str, output: &std::process::Output) -> String {
    format!("git {what}: {}", String::from_utf8_lossy(&output.stderr).trim())
}

fn container(repo: &Path) -> Option<(PathBuf, PathBuf)> {
    let common = git::stdout(repo, &["rev-parse", "--path-format=absolute", "--git-common-dir"], |_| String::new()).ok()?;
    let common = PathBuf::from(String::from_utf8_lossy(&common).trim());
    let container = common.parent()?.parent()?.to_path_buf();
    container.join("worktrees").is_dir().then_some((container, common))
}

#[derive(Debug, Default)]
struct Options {
    dry_run: bool,
    body_file: Option<String>,
    issue: Option<String>,
    repo: Option<String>,
    main: Option<PathBuf>,
    only: Option<String>,
    confined: bool,
}

const USAGE: &str = "north task check --done-when [--dry-run] [--confined] [--main DIR] [--box TEXT] (--body-file FILE | REPO#N | --repo OWNER/REPO)";
const BROKERED: &str = "inside a worker sandbox the outside broker runs the check that writes a verdict, for one box of one issue: north task check --done-when --box TEXT REPO#N; add --dry-run to read the verdicts and write nothing";

/// What a `task check --done-when` asked inside a worker sandbox, whose domain keeps the ledger and the checker's lock unwritable, becomes.
#[derive(Debug, Eq, PartialEq)]
pub enum Sandboxed {
    InPlace,
    Brokered(Vec<String>),
    Refused(String),
}

/// A run that writes no verdict (`--dry-run`, `--body-file`) stays in place; one box of one issue is the outside broker's to check, asked in the words returned; any other run would end at its first write and is refused before it starts.
pub fn sandboxed(arguments: &[String]) -> Sandboxed {
    let Ok(options) = options(arguments) else { return Sandboxed::InPlace };
    if options.dry_run || options.body_file.is_some() {
        return Sandboxed::InPlace;
    }
    match (&options.only, options.issue.as_deref().map(issue_reference), &options.main, options.confined) {
        (Some(only), Some(Ok((repo, number))), None, false) => Sandboxed::Brokered(["task", "check", "--done-when", "--box", only, &format!("{repo}#{number}")].map(str::to_owned).to_vec()),
        _ => Sandboxed::Refused(BROKERED.into()),
    }
}

fn options(arguments: &[String]) -> NorthResult<Options> {
    let usage = || NorthError::Usage(USAGE.into());
    let mut found = Options::default();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--done-when" => {}
            "--dry-run" => found.dry_run = true,
            "--confined" => found.confined = true,
            "--body-file" => found.body_file = Some(words.next().ok_or_else(usage)?.clone()),
            "--repo" => found.repo = Some(words.next().ok_or_else(usage)?.clone()),
            "--main" => found.main = Some(PathBuf::from(words.next().ok_or_else(usage)?)),
            "--box" => found.only = Some(words.next().ok_or_else(usage)?.clone()),
            reference if reference.contains('#') => found.issue = Some(reference.to_owned()),
            _ => return Err(usage()),
        }
    }
    if [&found.body_file, &found.issue, &found.repo].iter().filter(|source| source.is_some()).count() != 1 {
        return Err(usage());
    }
    Ok(found)
}

fn slug(repo: &str) -> String {
    if repo.contains('/') { repo.to_owned() } else { format!("tompassarelli/{repo}") }
}

fn issue_reference(reference: &str) -> NorthResult<(String, u64)> {
    let usage = || NorthError::Usage("REPO#N".into());
    let (repo, number) = reference.split_once('#').ok_or_else(usage)?;
    Ok((slug(repo), number.parse().map_err(|_| usage())?))
}

fn issues(repo: &str, reference: Option<&str>) -> NorthResult<Vec<(u64, String)>> {
    if let Some(reference) = reference {
        let (_, number) = issue_reference(reference)?;
        return Ok(vec![(number, Gh::of(repo, &[]).body(number).map_err(NorthError::Refused)?)]);
    }
    let listed = github::json(&["issue", "list", "-R", repo, "--state", "open", "-L", "500", "--json", "number,body"]).map_err(NorthError::Refused)?;
    Ok(listed
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|issue| Some((issue["number"].as_u64()?, issue["body"].as_str().unwrap_or_default().to_owned())))
        .filter(|(_, body)| !checks(body).is_empty())
        .collect())
}

/// `body` with every open box that does not contain `only` left out, so only the named box is checked.
fn only(body: &str, only: Option<&str>) -> String {
    body.lines().filter(|line| only.is_none_or(|text| !line.trim_start().starts_with(OPEN) || line.contains(text))).collect::<Vec<_>>().join("\n")
}

fn main_for(options: &Options) -> NorthResult<Main> {
    match options.main.clone() {
        Some(dir) => Ok(Main::Given(dir)),
        None => Main::fetch(&std::env::current_dir()?),
    }
}

/// `domain` makes the command that runs a box's shell command in a new worker domain, which `--confined` asks for: the broker's check for a session (north#1137) runs a command whose text a session may have written into the issue.
pub fn run(ledger: &Ledger, arguments: &[String], domain: &dyn Fn(&str) -> Command) -> NorthResult<()> {
    let options = options(arguments)?;
    let shell = |command: &str| if options.confined { domain(command) } else { plain(command) };
    if let Some(path) = &options.body_file {
        let main = main_for(&options)?;
        let stamp = Stamp { sha: main.sha()?, date: Timestamp::now().iso_date() };
        let run = |command: &str| judge_under(shell(command), main.dir());
        let body = std::fs::read_to_string(path)?;
        let verdicts = verdicts(&body, &run);
        for verdict in &verdicts {
            eprintln!("{}", verdict.line(path));
        }
        print!("{}", ticked(&body, &verdicts, &stamp));
        return Ok(());
    }
    let repo = match &options.issue {
        Some(reference) => issue_reference(reference)?.0,
        None => slug(options.repo.as_deref().unwrap_or_default()),
    };
    let mut tracker = Gh::of(&repo, &[]);
    let issues: Vec<(u64, String)> = issues(&repo, options.issue.as_deref())?.into_iter().map(|(number, body)| (number, only(&body, options.only.as_deref()))).filter(|(_, body)| !checks(body).is_empty()).collect();
    if issues.is_empty() {
        return Ok(());
    }
    let main = main_for(&options)?;
    let stamp = Stamp { sha: main.sha()?, date: Timestamp::now().iso_date() };
    let run = |command: &str| judge_under(shell(command), main.dir());
    for (number, body) in issues {
        let verdicts = verdicts(&body, &run);
        for verdict in &verdicts {
            println!("{}", verdict.line(&format!("{repo}#{number}")));
        }
        if !options.dry_run {
            record(ledger, &format!("{}#{number}", repo.rsplit('/').next().unwrap_or(&repo)), &verdicts, main.dir(), &stamp)?;
            settle(&mut tracker, number, &verdicts, &stamp).map_err(NorthError::Refused)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
