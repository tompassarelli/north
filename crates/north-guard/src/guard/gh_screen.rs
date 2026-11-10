use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};

const ALLOW: &str = "--allow-shrink";
const VALUELESS: [&str; 3] = ["--remove-milestone", "--help", "-h"];

#[derive(Debug, PartialEq, Eq)]
enum NewBody {
    Text(String),
    File(PathBuf),
    Stdin,
}

#[derive(Debug, PartialEq, Eq)]
struct IssueEdit {
    selectors: Vec<String>,
    repo: Option<String>,
    body: NewBody,
}

trait World {
    fn current(&mut self, selector: &str, repo: Option<&str>) -> Option<String>;
    fn stdin(&mut self) -> std::io::Result<Vec<u8>>;
}

struct Live<'a> {
    command: &'a Command,
}

impl World for Live<'_> {
    fn current(&mut self, selector: &str, repo: Option<&str>) -> Option<String> {
        let mut view = Command::new(self.command.get_program());
        view.args(["issue", "view", selector, "--json", "body", "-q", ".body"]).args(repo.map(|repo| ["-R", repo]).into_iter().flatten()).stdin(Stdio::null());
        for (name, value) in self.command.get_envs() {
            match value {
                Some(value) => view.env(name, value),
                None => view.env_remove(name),
            };
        }
        let out = view.output().ok().filter(|out| out.status.success())?;
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn stdin(&mut self) -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        std::io::stdin().read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

/// Run by `north gh-session` on every gh call, so the refusal holds for each provider and for a body file the same command wrote.
pub fn screen(command: &mut Command) -> Result<(), String> {
    let arguments: Vec<String> = command.get_args().map(|word| word.to_string_lossy().into_owned()).collect();
    let Some((kept, allowed, edit)) = parse(&arguments) else {
        return Ok(());
    };
    let stdin = judge(&edit, allowed.as_deref(), &mut Live { command })?;
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

fn judge(edit: &IssueEdit, allowed: Option<&str>, world: &mut impl World) -> Result<Option<Vec<u8>>, String> {
    let (new, stdin) = match &edit.body {
        NewBody::Text(text) => (text.clone(), None),
        NewBody::File(path) => (std::fs::read_to_string(path).unwrap_or_default(), None),
        NewBody::Stdin => {
            let bytes = world.stdin().map_err(|error| format!("cannot read the body from stdin: {error}"))?;
            (String::from_utf8_lossy(&bytes).into_owned(), Some(bytes))
        }
    };
    if allowed.is_some() {
        return Ok(stdin);
    }
    for selector in &edit.selectors {
        let Some(old) = world.current(selector, edit.repo.as_deref()) else {
            continue;
        };
        if shrinks(&old, &new) {
            return Err(refusal(selector, &old, &new));
        }
    }
    Ok(stdin)
}

fn parse(arguments: &[String]) -> Option<(Vec<String>, Option<String>, IssueEdit)> {
    let mut kept = Vec::with_capacity(arguments.len());
    let mut allowed = None;
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        if word == "--" {
            kept.push(word.clone());
            kept.extend(words.by_ref().cloned());
        } else if word == ALLOW {
            allowed = words.next().cloned();
        } else if let Some(reason) = word.strip_prefix("--allow-shrink=") {
            allowed = Some(reason.to_owned());
        } else {
            kept.push(word.clone());
        }
    }
    let edit = issue_edit(&kept)?;
    Some((kept, allowed.filter(|reason| !reason.trim().is_empty()), edit))
}

fn issue_edit(arguments: &[String]) -> Option<IssueEdit> {
    let [group, verb, rest @ ..] = arguments else {
        return None;
    };
    if group != "issue" || verb != "edit" {
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
    Some(IssueEdit { selectors, repo, body: body? })
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

fn refusal(selector: &str, old: &str, new: &str) -> String {
    let (old, new) = (old.trim().chars().count(), new.trim().chars().count());
    format!(
        "refusing gh issue edit {selector}: it replaces a {old}-character body with {new} characters, which blanks or drops most of it. Append to the body you read (gh issue view {selector} --json body -q .body > FILE, then add your lines) or comment instead; when the shrink is meant, pass {ALLOW} REASON to gh issue edit."
    )
}

#[cfg(test)]
#[path = "gh_screen_tests.rs"]
mod tests;
