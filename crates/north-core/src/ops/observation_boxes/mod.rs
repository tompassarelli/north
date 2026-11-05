use std::process::Command;

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;
use crate::platform::github;
use crate::platform::rolling_issue::{Gh, Tracker};

const MARKER: &str = "Observe: `";
const OPEN: &str = "- [ ] ";

#[derive(Debug, Eq, PartialEq)]
pub struct Observation {
    pub line: usize,
    pub text: String,
    pub command: String,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Observed {
    pub observation: Observation,
    pub exit: i32,
}

pub fn observations(body: &str) -> Vec<Observation> {
    body.lines()
        .enumerate()
        .filter_map(|(line, text)| {
            let rest = text.trim_start().strip_prefix(OPEN)?;
            let after = &rest[rest.find(MARKER)? + MARKER.len()..];
            let command = after[..after.find('`')?].trim();
            (!command.is_empty()).then(|| Observation { line, text: rest.trim().to_owned(), command: command.to_owned() })
        })
        .collect()
}

pub fn observe(command: &str) -> i32 {
    match Command::new("sh").args(["-c", command]).output() {
        Ok(output) => output.status.code().unwrap_or(1),
        Err(_) => 127,
    }
}

pub fn ticked(body: &str, observed: &[Observed], date: &str) -> String {
    let mut lines: Vec<String> = body.lines().map(str::to_owned).collect();
    for run in observed.iter().filter(|run| run.exit == 0) {
        let line = &mut lines[run.observation.line];
        *line = format!("{} Evidence: `{}` exited 0 on {date}.", line.replacen("- [ ]", "- [x]", 1), run.observation.command);
    }
    let mut text = lines.join("\n");
    if body.ends_with('\n') {
        text.push('\n');
    }
    text
}

pub fn settle_body(body: &str, run: &dyn Fn(&str) -> i32, date: &str) -> (String, Vec<Observed>) {
    let observed: Vec<Observed> = observations(body).into_iter().map(|observation| Observed { exit: run(&observation.command), observation }).collect();
    (ticked(body, &observed, date), observed)
}

struct Options {
    dry_run: bool,
    body_file: Option<String>,
    issue: Option<String>,
    repo: Option<String>,
}

fn options(arguments: &[String]) -> NorthResult<Options> {
    let usage = || NorthError::Usage("north task check [--observations] [--dry-run] (--body-file FILE | REPO#N | --repo OWNER/REPO)".into());
    let mut found = Options { dry_run: false, body_file: None, issue: None, repo: None };
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--observations" => {}
            "--dry-run" => found.dry_run = true,
            "--body-file" => found.body_file = Some(words.next().ok_or_else(usage)?.clone()),
            "--repo" => found.repo = Some(words.next().ok_or_else(usage)?.clone()),
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

fn report(label: &str, observed: &[Observed]) {
    for run in observed {
        let verdict = if run.exit == 0 { "ticked" } else { "open" };
        eprintln!("{label}: {verdict} (exit {}): {}", run.exit, run.observation.text);
    }
}

fn settle_issue(repo: &str, number: u64, body: &str, dry_run: bool, date: &str) -> NorthResult<()> {
    let (edited, observed) = settle_body(body, &observe, date);
    let label = format!("{repo}#{number}");
    report(&label, &observed);
    if dry_run {
        print!("{edited}");
    } else if edited != body {
        Gh::of(repo, &[]).edit(number, &edited).map_err(NorthError::Refused)?;
    }
    Ok(())
}

pub fn run(arguments: &[String]) -> NorthResult<()> {
    let options = options(arguments)?;
    let date = Timestamp::now().iso_date();
    if let Some(path) = &options.body_file {
        let body = std::fs::read_to_string(path)?;
        let (edited, observed) = settle_body(&body, &observe, &date);
        report(path, &observed);
        print!("{edited}");
        return Ok(());
    }
    if let Some(reference) = &options.issue {
        let (repo, number) = reference.split_once('#').ok_or_else(|| NorthError::Usage("REPO#N".into()))?;
        let number: u64 = number.parse().map_err(|_| NorthError::Usage("REPO#N".into()))?;
        let repo = slug(repo);
        let issue = github::json(&["issue", "view", &number.to_string(), "-R", &repo, "--json", "body"]).map_err(NorthError::Refused)?;
        return settle_issue(&repo, number, issue["body"].as_str().unwrap_or_default(), options.dry_run, &date);
    }
    let repo = slug(options.repo.as_deref().unwrap_or_default());
    let listed = github::json(&["issue", "list", "-R", &repo, "--state", "open", "-L", "500", "--json", "number,body"]).map_err(NorthError::Refused)?;
    for issue in listed.as_array().into_iter().flatten() {
        let body = issue["body"].as_str().unwrap_or_default();
        if let Some(number) = issue["number"].as_u64().filter(|_| !observations(body).is_empty()) {
            settle_issue(&repo, number, body, options.dry_run, &date)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
