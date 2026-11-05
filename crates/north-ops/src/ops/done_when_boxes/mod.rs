use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use tempfile::TempDir;

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;
use crate::platform::rolling_issue::{Gh, Tracker};
use crate::platform::{git, github, process};

const MARKER: &str = "Done when: `";
const OPEN: &str = "- [ ] ";
const LIMIT: Duration = Duration::from_secs(600);
const KEPT_LINES: usize = 20;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Check {
    pub text: String,
    pub command: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Verdict {
    pub check: Check,
    pub exit: i32,
    pub output: String,
}

impl Verdict {
    pub fn passed(&self) -> bool {
        self.exit == 0
    }

    pub fn line(&self, label: &str) -> String {
        let verdict = if self.passed() { "pass" } else { "fail" };
        format!("{label}: {verdict} (exit {}) `{}`: {}", self.exit, self.check.command, self.check.text)
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
            let command = after[..after.find('`')?].trim();
            (!command.is_empty()).then(|| Check { text: rest.trim().to_owned(), command: command.to_owned() })
        })
        .collect()
}

pub fn judge(command: &str, main: &Path) -> (i32, String) {
    let mut shell = Command::new("sh");
    shell.args(["-c", command]).current_dir(main);
    match process::run_group_within(shell, LIMIT) {
        Some(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            (output.status.code().unwrap_or(1), text)
        }
        None => (124, format!("no exit within {}s", LIMIT.as_secs())),
    }
}

pub fn verdicts(body: &str, run: &dyn Fn(&str) -> (i32, String)) -> Vec<Verdict> {
    checks(body)
        .into_iter()
        .map(|check| {
            let (exit, output) = run(&check.command);
            Verdict { check, exit, output }
        })
        .collect()
}

fn attachment(output: &str) -> Vec<String> {
    let lines: Vec<&str> = output.trim_end().lines().collect();
    if lines.iter().all(|line| line.trim().is_empty()) {
        return Vec::new();
    }
    let kept = &lines[lines.len().saturating_sub(KEPT_LINES)..];
    let fence = if output.contains("```") { "````" } else { "```" };
    let mut block = vec![format!("  {fence}text")];
    block.extend(kept.iter().map(|line| format!("  {line}").trim_end().to_owned()));
    block.push(format!("  {fence}"));
    block
}

pub fn ticked(body: &str, verdicts: &[Verdict], stamp: &Stamp) -> String {
    let mut lines: Vec<String> = body.lines().map(str::to_owned).collect();
    for verdict in verdicts.iter().filter(|verdict| verdict.passed()) {
        let open = format!("{OPEN}{}", verdict.check.text);
        let Some(index) = lines.iter().position(|line| line.trim() == open) else { continue };
        let indent: String = lines[index].chars().take_while(|character| character.is_whitespace()).collect();
        let ticked = lines[index].trim_end().replacen("- [ ]", "- [x]", 1);
        let short = &stamp.sha[..stamp.sha.len().min(12)];
        lines[index] = format!("{ticked} Evidence: `{}` exited 0 on main {short} on {}.", verdict.check.command, stamp.date);
        let block: Vec<String> = attachment(&verdict.output).into_iter().map(|line| format!("{indent}{line}")).collect();
        lines.splice(index + 1..index + 1, block);
    }
    let mut text = lines.join("\n");
    if body.ends_with('\n') {
        text.push('\n');
    }
    text
}

pub fn settle(tracker: &mut dyn Tracker, number: u64, body: &str, run: &dyn Fn(&str) -> (i32, String), stamp: &Stamp, dry_run: bool) -> Result<Vec<Verdict>, String> {
    let verdicts = verdicts(body, run);
    if dry_run || !verdicts.iter().any(Verdict::passed) {
        return Ok(verdicts);
    }
    let fresh = tracker.body(number)?;
    let edited = ticked(&fresh, &verdicts, stamp);
    if edited != fresh {
        tracker.edit(number, &edited)?;
    }
    Ok(verdicts)
}

pub enum Main {
    Given(PathBuf),
    Fetched { repo: PathBuf, checkout: TempDir },
}

impl Main {
    pub fn fetch(repo: &Path) -> NorthResult<Self> {
        let failed = |what: &str, output: &std::process::Output| format!("git {what}: {}", String::from_utf8_lossy(&output.stderr).trim());
        git::stdout(repo, &["fetch", "--quiet", "origin", "main"], |output| failed("fetch origin main", output)).map_err(NorthError::Refused)?;
        let checkout = tempfile::Builder::new().prefix("north-done-when-").tempdir()?;
        let path = checkout.path().to_string_lossy().into_owned();
        git::stdout(repo, &["worktree", "add", "--quiet", "--detach", &path, "origin/main"], |output| failed("worktree add", output)).map_err(NorthError::Refused)?;
        Ok(Self::Fetched { repo: repo.to_owned(), checkout })
    }

    pub fn dir(&self) -> &Path {
        match self {
            Self::Given(dir) => dir,
            Self::Fetched { checkout, .. } => checkout.path(),
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
        if let Self::Fetched { repo, checkout } = self {
            let _ = git::run(repo, &["worktree", "remove", "--force", &checkout.path().to_string_lossy()]);
        }
    }
}

#[derive(Debug, Default)]
struct Options {
    dry_run: bool,
    body_file: Option<String>,
    issue: Option<String>,
    repo: Option<String>,
    main: Option<PathBuf>,
}

const USAGE: &str = "north task check --done-when [--dry-run] [--main DIR] (--body-file FILE | REPO#N | --repo OWNER/REPO)";

fn options(arguments: &[String]) -> NorthResult<Options> {
    let usage = || NorthError::Usage(USAGE.into());
    let mut found = Options::default();
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--done-when" => {}
            "--dry-run" => found.dry_run = true,
            "--body-file" => found.body_file = Some(words.next().ok_or_else(usage)?.clone()),
            "--repo" => found.repo = Some(words.next().ok_or_else(usage)?.clone()),
            "--main" => found.main = Some(PathBuf::from(words.next().ok_or_else(usage)?)),
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

pub fn run(arguments: &[String]) -> NorthResult<()> {
    let options = options(arguments)?;
    let main = match options.main.clone() {
        Some(dir) => Main::Given(dir),
        None => Main::fetch(&std::env::current_dir()?)?,
    };
    let stamp = Stamp { sha: main.sha()?, date: Timestamp::now().iso_date() };
    let run = |command: &str| judge(command, main.dir());
    if let Some(path) = &options.body_file {
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
    for (number, body) in issues(&repo, options.issue.as_deref())? {
        let verdicts = settle(&mut tracker, number, &body, &run, &stamp, options.dry_run).map_err(NorthError::Refused)?;
        for verdict in &verdicts {
            println!("{}", verdict.line(&format!("{repo}#{number}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
