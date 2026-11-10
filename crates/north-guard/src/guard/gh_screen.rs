use super::unrunnable_box::{self, Filing};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const ALLOW: &str = "--allow-shrink";
const ALLOW_GROWTH: &str = "--allow-growth";
const REPEATED_BOX_CHARS: usize = 80;
const VALUELESS: [&str; 3] = ["--remove-milestone", "--help", "-h"];

#[derive(Debug, PartialEq, Eq)]
enum NewBody {
    Text(String),
    File(PathBuf),
    Stdin,
}

#[derive(Debug, PartialEq, Eq)]
struct IssueWrite {
    creates: bool,
    selectors: Vec<String>,
    repo: Option<String>,
    body: NewBody,
    growth_allowed: bool,
}

trait World: unrunnable_box::Log {
    fn current(&mut self, selector: &str, repo: Option<&str>) -> Option<String>;
    fn repo(&mut self) -> Option<String>;
    fn stdin(&mut self) -> std::io::Result<Vec<u8>>;
}

struct Live<'a> {
    command: &'a Command,
}

impl Live<'_> {
    fn gh(&self, arguments: &[&str]) -> Option<String> {
        let mut view = Command::new(self.command.get_program());
        view.args(arguments).stdin(Stdio::null());
        if let Some(dir) = self.command.get_current_dir() {
            view.current_dir(dir);
        }
        for (name, value) in self.command.get_envs() {
            match value {
                Some(value) => view.env(name, value),
                None => view.env_remove(name),
            };
        }
        let out = view.output().ok().filter(|out| out.status.success())?;
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

impl unrunnable_box::Log for Live<'_> {
    fn enforcing(&self) -> bool {
        unrunnable_box::Live.enforcing()
    }

    fn record(&mut self, row: serde_json::Value) {
        unrunnable_box::Live.record(row);
    }
}

impl World for Live<'_> {
    fn current(&mut self, selector: &str, repo: Option<&str>) -> Option<String> {
        let mut arguments = vec!["issue", "view", selector, "--json", "body", "-q", ".body"];
        arguments.extend(repo.map(|repo| ["-R", repo]).into_iter().flatten());
        self.gh(&arguments)
    }

    fn repo(&mut self) -> Option<String> {
        self.gh(&["repo", "view", "--json", "nameWithOwner", "-q", ".nameWithOwner"]).map(|out| out.trim().to_owned())
    }

    fn stdin(&mut self) -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

/// Run by `north gh-session` on every gh call, so its refusals and the unrunnable-box gate hold for each provider and for a body file the same command wrote.
pub fn screen(command: &mut Command) -> Result<(), String> {
    let arguments: Vec<String> = command.get_args().map(|word| word.to_string_lossy().into_owned()).collect();
    let Some((kept, allowed, write)) = parse(&arguments) else {
        return Ok(());
    };
    let stdin = judge(&write, allowed.as_deref(), &mut Live { command })?;
    let mut fresh = Command::new(command.get_program());
    fresh.args(kept);
    for (name, value) in command.get_envs() {
        match value {
            Some(value) => fresh.env(name, value),
            None => fresh.env_remove(name),
        };
    }
    if let Some(bytes) = stdin {
        fresh.stdin(replay(&bytes).map_err(|error| format!("cannot keep the body read from stdin: {error}"))?);
    }
    *command = fresh;
    Ok(())
}

fn replay(bytes: &[u8]) -> std::io::Result<std::fs::File> {
    use std::io::{Seek, Write};
    let mut file = tempfile::tempfile()?;
    file.write_all(bytes)?;
    file.rewind()?;
    Ok(file)
}

fn judge(write: &IssueWrite, allowed: Option<&str>, world: &mut impl World) -> Result<Option<Vec<u8>>, String> {
    let (new, stdin) = match &write.body {
        NewBody::Text(text) => (text.clone(), None),
        NewBody::File(path) => (std::fs::read_to_string(path).unwrap_or_default(), None),
        NewBody::Stdin => {
            let bytes = world.stdin().map_err(|error| format!("cannot read the body from stdin: {error}"))?;
            (String::from_utf8_lossy(&bytes).into_owned(), Some(bytes))
        }
    };
    let mut held = Vec::new();
    for selector in write.selectors.iter().filter(|_| !write.creates) {
        let Some(old) = world.current(selector, write.repo.as_deref()) else {
            if allowed.is_none() {
                return Err(unreadable(selector));
            }
            continue;
        };
        if allowed.is_none() && shrinks(&old, &new) {
            return Err(refusal(selector, &old, &new));
        }
        runaway(write.growth_allowed, selector, &old, &new)?;
        held.push((selector, old));
    }
    let filings: Vec<Filing> = if write.creates { vec![Filing { issue: None, held: None }] } else { held.iter().map(|(selector, old)| Filing { issue: Some(selector.as_str()), held: Some(old.as_str()) }).collect() };
    if let Some(repo) = write.repo.clone().or_else(|| world.repo()).filter(|_| !filings.is_empty()) {
        unrunnable_box::judge(&repo, &filings, &new, world)?;
    }
    Ok(stdin)
}

fn parse(arguments: &[String]) -> Option<(Vec<String>, Option<String>, IssueWrite)> {
    let mut kept = Vec::with_capacity(arguments.len());
    let (mut allowed, mut growth_allowed) = (None, false);
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        if word == "--" {
            kept.push(word.clone());
            kept.extend(words.by_ref().cloned());
        } else if word == ALLOW {
            allowed = words.next().cloned();
        } else if let Some(reason) = word.strip_prefix("--allow-shrink=") {
            allowed = Some(reason.to_owned());
        } else if let Some(reason) = growth_reason(word, &mut words) {
            growth_allowed = !reason.trim().is_empty();
        } else {
            kept.push(word.clone());
        }
    }
    let mut write = issue_write(&kept)?;
    write.growth_allowed = growth_allowed;
    Some((kept, allowed.filter(|reason| !reason.trim().is_empty()), write))
}

fn growth_reason(word: &str, words: &mut std::slice::Iter<'_, String>) -> Option<String> {
    if word == ALLOW_GROWTH {
        return Some(words.next().cloned().unwrap_or_default());
    }
    word.strip_prefix("--allow-growth=").map(str::to_owned)
}

fn issue_write(arguments: &[String]) -> Option<IssueWrite> {
    let [group, verb, rest @ ..] = arguments else {
        return None;
    };
    if group != "issue" || !matches!(verb.as_str(), "create" | "edit") {
        return None;
    }
    let (mut selectors, mut repo, mut body) = (Vec::new(), None, None);
    for (name, value) in tokens(rest) {
        match name {
            None => selectors.extend(value),
            Some("--body" | "-b") => body = value.map(NewBody::Text),
            Some("--body-file" | "-F") => body = value.map(body_file),
            Some("--repo" | "-R") => repo = value,
            Some(_) => {}
        }
    }
    Some(IssueWrite { creates: verb == "create", selectors, repo, body: body?, growth_allowed: false })
}

fn body_file(file: String) -> NewBody {
    if file == "-" { NewBody::Stdin } else { NewBody::File(PathBuf::from(file)) }
}

fn tokens(rest: &[String]) -> Vec<(Option<&str>, Option<String>)> {
    let mut tokens = Vec::new();
    let mut words = rest.iter();
    while let Some(word) = words.next() {
        if word == "--" {
            tokens.extend(words.by_ref().map(|word| (None, Some(word.clone()))));
        } else if !word.starts_with('-') || word == "-" {
            tokens.push((None, Some(word.clone())));
        } else {
            let (name, value) = flag(word, &mut words);
            tokens.push((Some(name), value));
        }
    }
    tokens
}

fn flag<'a>(word: &'a str, rest: &mut std::slice::Iter<'_, String>) -> (&'a str, Option<String>) {
    if let Some((name, value)) = word.split_once('=').filter(|_| word.starts_with("--")) {
        return (name, Some(value.to_owned()));
    }
    if !word.starts_with("--") && word.len() > 2 {
        return (&word[..2], Some(word[2..].to_owned()));
    }
    if VALUELESS.contains(&word) {
        return (word, None);
    }
    (word, rest.next().cloned())
}

pub fn shrinks(old: &str, new: &str) -> bool {
    let (old, new) = (old.trim().chars().count(), new.trim().chars().count());
    new == 0 && old > 0 || new * 2 < old
}

fn runaway(allowed: bool, selector: &str, old: &str, new: &str) -> Result<(), String> {
    if allowed {
        return Ok(());
    }
    if let Some(line) = repeated_box(new) {
        return Err(repetition(selector, &line));
    }
    if grows(old, new) {
        return Err(growth(selector, old, new));
    }
    Ok(())
}

fn grows(old: &str, new: &str) -> bool {
    new.trim().chars().count() > old.trim().chars().count() * 2
}

fn repeated_box(body: &str) -> Option<String> {
    let mut seen = std::collections::HashSet::new();
    body.lines()
        .map(str::trim)
        .filter(|line| ["- [ ]", "- [x]", "- [X]"].iter().any(|mark| line.starts_with(mark)) && line.chars().count() >= REPEATED_BOX_CHARS)
        .find(|line| !seen.insert(*line))
        .map(str::to_owned)
}

fn repetition(selector: &str, line: &str) -> String {
    let shown: String = line.chars().take(60).collect();
    format!(
        "refusing gh issue edit {selector}: the new body repeats the box line \"{shown}...\", which a `sed -i \"Nr FILE\"` with an empty line number does by appending after every line. Re-read the body (gh issue view {selector} --json body -q .body > FILE), make one insertion, and check the file before editing; when the repeat is meant, pass {ALLOW_GROWTH} REASON to gh issue edit."
    )
}

fn growth(selector: &str, old: &str, new: &str) -> String {
    let (old, new) = (old.trim().chars().count(), new.trim().chars().count());
    format!(
        "refusing gh issue edit {selector}: it replaces a {old}-character body with {new} characters, more than double, which a runaway insertion does. Check the file you edit from (gh issue view {selector} --json body -q .body > FILE) for repeated lines; when the growth is meant, pass {ALLOW_GROWTH} REASON to gh issue edit."
    )
}

fn unreadable(selector: &str) -> String {
    format!(
        "refusing gh issue edit {selector}: the current body could not be read, so a body that blanks or drops most of it cannot be told from one that does not. Run the edit from a directory inside the repository or pass -R OWNER/REPO and retry; when the edit is meant anyway, pass {ALLOW} REASON to gh issue edit."
    )
}

fn refusal(selector: &str, old: &str, new: &str) -> String {
    let (old, new) = (old.trim().chars().count(), new.trim().chars().count());
    format!(
        "refusing gh issue edit {selector}: it replaces a {old}-character body with {new} characters, which blanks or drops most of it. Append to the body you read (gh issue view {selector} --json body -q .body > FILE, then add your lines) or comment instead; when the shrink is meant, pass {ALLOW} REASON to gh issue edit."
    )
}

#[cfg(test)]
#[path = "gh_screen_tests.rs"]
mod tests;
