//! Rewrites the cargo commands in open issue boxes onto Buck2 by the north#1115 rules, at the Buck2 cut-over.
mod command;
mod workspace;

use std::path::PathBuf;

use crate::error::{NorthError, NorthResult};
use crate::platform::github;
use crate::platform::rolling_issue::{Gh, Tracker};

pub use workspace::Workspace;

#[derive(Debug, PartialEq, Eq)]
pub struct Rewritten {
    pub before: String,
    pub after: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Refused {
    pub line: String,
    pub reason: String,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Rewrite {
    pub body: String,
    pub rewritten: Vec<Rewritten>,
    pub refused: Vec<Refused>,
}

fn open_box(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("- [ ]") || line.starts_with("* [ ]")
}

fn rewrite_line(line: &str, workspace: &Workspace) -> Result<Option<String>, String> {
    let pieces: Vec<&str> = line.split('`').collect();
    if pieces.len() < 3 {
        return Ok(None);
    }
    let mut changed = false;
    let mut rebuilt = Vec::with_capacity(pieces.len());
    for (index, piece) in pieces.iter().enumerate() {
        let span = index % 2 == 1 && index + 1 < pieces.len();
        if span && command::calls_cargo(piece) {
            rebuilt.push(command::rewrite(piece, workspace)?);
            changed = true;
        } else {
            rebuilt.push((*piece).to_owned());
        }
    }
    Ok(changed.then(|| rebuilt.join("`")))
}

pub fn rewrite_body(body: &str, workspace: &Workspace) -> Rewrite {
    let mut rewrite = Rewrite::default();
    let lines: Vec<String> = body
        .split('\n')
        .map(|line| {
            if !open_box(line) {
                return line.to_owned();
            }
            match rewrite_line(line, workspace) {
                Ok(Some(after)) => {
                    rewrite.rewritten.push(Rewritten { before: line.to_owned(), after: after.clone() });
                    after
                }
                Ok(None) => line.to_owned(),
                Err(reason) => {
                    rewrite.refused.push(Refused { line: line.to_owned(), reason });
                    line.to_owned()
                }
            }
        })
        .collect();
    rewrite.body = lines.join("\n");
    rewrite
}

struct Options {
    dry_run: bool,
    repo: String,
    tree: PathBuf,
}

fn options(arguments: &[String]) -> NorthResult<Options> {
    let usage = || NorthError::Usage("north task rewrite-cargo [--dry-run] [--repo OWNER/REPO] [--tree DIR]".into());
    let mut found = Options { dry_run: false, repo: "tompassarelli/north".to_owned(), tree: PathBuf::from(".") };
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--dry-run" => found.dry_run = true,
            "--repo" => found.repo = words.next().ok_or_else(usage)?.clone(),
            "--tree" => found.tree = PathBuf::from(words.next().ok_or_else(usage)?),
            _ => return Err(usage()),
        }
    }
    Ok(found)
}

fn body_of(issue: &serde_json::Value) -> &str {
    issue["body"].as_str().unwrap_or_default()
}

pub fn run(arguments: &[String]) -> NorthResult<()> {
    let options = options(arguments)?;
    let workspace = Workspace::read(&options.tree).map_err(NorthError::Refused)?;
    let listed = github::json(&["issue", "list", "-R", &options.repo, "--state", "open", "-L", "1000", "--json", "number,body"]).map_err(NorthError::Refused)?;
    let mut changed = Vec::new();
    let (mut boxes, mut refusals) = (0, 0);
    for issue in listed.as_array().into_iter().flatten() {
        let Some(number) = issue["number"].as_u64() else { continue };
        let rewrite = rewrite_body(body_of(issue), &workspace);
        for Rewritten { before, after } in &rewrite.rewritten {
            println!("{}#{number}\n- {}\n+ {}\n", options.repo, before.trim(), after.trim());
        }
        for Refused { line, reason } in &rewrite.refused {
            println!("{}#{number} refused: {reason}\n  {}\n", options.repo, line.trim());
        }
        boxes += rewrite.rewritten.len();
        refusals += rewrite.refused.len();
        if !rewrite.rewritten.is_empty() {
            changed.push(number);
        }
    }
    eprintln!("{boxes} boxes rewritten on {} issues, {refusals} refused", changed.len());
    if refusals > 0 {
        return Err(NorthError::Refused(format!("{refusals} boxes call cargo in a shape with no rule; rewrite them by hand or add the rule, then run again (nothing was edited)")));
    }
    if options.dry_run {
        return Ok(());
    }
    let mut tracker = Gh::of(&options.repo, &[]);
    for number in changed {
        let issue = github::json(&["issue", "view", &number.to_string(), "-R", &options.repo, "--json", "body"]).map_err(NorthError::Refused)?;
        let rewrite = rewrite_body(body_of(&issue), &workspace);
        if !rewrite.refused.is_empty() {
            return Err(NorthError::Refused(format!("{}#{number} changed since it was listed and now has a box with no rule", options.repo)));
        }
        if rewrite.body != body_of(&issue) {
            tracker.edit(number, &rewrite.body).map_err(NorthError::Refused)?;
            eprintln!("edited {}#{number}", options.repo);
        }
    }
    Ok(())
}
