//! Boxes deferred to a dated check: a box whose code landed and whose only remaining check is a
//! measurement over time (a day of logs, N ledger runs) names a command in the registry's `[[check]]`
//! table and waits on its issue with a marker. On the review day the loop runs the command, ticks the
//! box with the evidence on a pass and reopens it with the output on a miss, telling the lead that
//! deferred it. An issue whose only unticked boxes wait on checks the landed registry holds closes on
//! the next tick, since its work has landed.
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::file::{Filed, Issues};
use super::registry::{self, Check, Day, Issue};
use crate::flow::boxes;
use crate::platform::box_oracles;
use crate::platform::process::run_group_within;

const FROM: &str = "service:north-experiments";
const NO_LEAD: &str = "proxy";

/// The sessions told when a check misses.
pub trait Leads {
    fn tell(&self, lead: &str, text: &str) -> Result<(), String>;
}

/// Leads through `north fleet send`.
pub struct Fleet;

impl Leads for Fleet {
    fn tell(&self, lead: &str, text: &str) -> Result<(), String> {
        let output = Command::new("north").args(["fleet", "send", "--from", FROM, lead, text]).output().map_err(|why| format!("cannot run north fleet send: {why}"))?;
        output.status.success().then_some(()).ok_or_else(|| format!("north fleet send {lead}: {}", String::from_utf8_lossy(&output.stderr).trim()))
    }
}

/// Where a judging run files what it found.
pub struct Filing<'a> {
    pub issues: &'a dyn Issues,
    pub filed: &'a mut Filed,
    pub leads: &'a dyn Leads,
}

const CHECK_WITHIN: Duration = Duration::from_secs(120);
const OUTPUT_CHARS: usize = 300;
const NOT_RUN: [i32; 2] = [126, 127];

/// What running a check's command found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ran {
    Passed(String),
    Missed(String),
    Unrun(String),
}

fn key(check: &Check) -> String {
    format!("{}@{}", check.id, check.review.text())
}

fn pending(check: &Check) -> String {
    format!("<!-- check {} -->", key(check))
}

fn settled(check: &Check, word: &str) -> String {
    format!("<!-- check {} {word} -->", key(check))
}

fn waits(check: &Check) -> String {
    format!(" (waits: {}, deferred to {} check {})", check.review.text(), registry::PATH, check.id)
}

fn brief(text: &str) -> String {
    let flat = registry::collapse(text);
    if flat.chars().count() <= OUTPUT_CHARS { flat } else { format!("{}...", flat.chars().take(OUTPUT_CHARS).collect::<String>()) }
}

/// `body` with its first line equal to `line` replaced.
fn replace_line(body: &str, line: &str, with: &str) -> String {
    let mut replaced = false;
    let lines: Vec<&str> = body
        .lines()
        .map(|each| if !replaced && each == line { replaced = true; with } else { each })
        .collect();
    let mut text = lines.join("\n");
    if body.ends_with('\n') {
        text.push('\n');
    }
    text
}

fn closed(check: &Check) -> String {
    format!("closed {}", key(check))
}

/// The body with the one unticked box naming `check.text` marked as waiting for the check's review day.
pub fn deferred_body(body: &str, check: &Check) -> Result<String, String> {
    if body.contains(&pending(check)) {
        return Err(format!("{} already waits on check {}", check.owner.text(), check.id));
    }
    let wanted = registry::collapse(&check.text);
    let found: Vec<&str> = boxes::unticked(body).filter(|line| registry::collapse(line).contains(&wanted)).collect();
    let line = match found.as_slice() {
        [line] => *line,
        [] => return Err(format!("{} has no unticked box containing {:?}", check.owner.text(), check.text)),
        many => return Err(format!("{} has {} unticked boxes containing {:?}; name more of the box", check.owner.text(), many.len(), check.text)),
    };
    Ok(replace_line(body, line, &format!("{}{} {}", line.trim_end(), waits(check), pending(check))))
}

/// The body with the deferred box ticked and carrying the evidence on a pass, or left open and
/// carrying the output on a miss.
pub fn settled_body(body: &str, check: &Check, ran: &Ran, date: &str) -> Result<String, String> {
    let marker = pending(check);
    let line = body.lines().find(|line| line.contains(&marker)).ok_or_else(|| format!("{} has no box waiting on check {}", check.owner.text(), check.id))?;
    let bare = line.replace(&marker, "").replace(&waits(check), "");
    let bare = bare.trim_end();
    let command = &check.command;
    let rewritten = match ran {
        Ran::Passed(output) => format!("{} Evidence: `{command}` passed on {date}: {output} {}", bare.replacen("[ ]", "[x]", 1), settled(check, "passed")),
        Ran::Missed(output) => format!("{bare} Check `{command}` missed on {date}: {output} {}", settled(check, "missed")),
        Ran::Unrun(why) => return Err(format!("check {} did not run: {why}", check.id)),
    };
    Ok(replace_line(body, line, &rewritten))
}

/// Runs the check's command from the repository root and reads its answer: it passes on exit 0 and,
/// when the entry names an expected output, on that output.
pub fn run(check: &Check, root: &Path) -> Ran {
    let mut command = Command::new("sh");
    command.args(["-c", &check.command]).current_dir(root);
    let Some(output) = run_group_within(command, CHECK_WITHIN) else {
        return Ran::Unrun(format!("did not finish within {} s", CHECK_WITHIN.as_secs()));
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(1);
    if NOT_RUN.contains(&code) {
        return Ran::Unrun(format!("exit {code}: {}", brief(&stderr)));
    }
    let printed = stdout.trim();
    let as_expected = check.expect.as_deref().is_none_or(|expect| printed == expect.trim());
    if code == 0 && as_expected {
        return Ran::Passed(format!("exit 0, printed {:?}", brief(&stdout)));
    }
    let wanted = check.expect.as_deref().map_or_else(String::new, |expect| format!(", expected {:?}", expect.trim()));
    Ran::Missed(format!("exit {code}, printed {:?}{wanted}{}", brief(&stdout), if stderr.trim().is_empty() { String::new() } else { format!(", stderr {:?}", brief(&stderr)) }))
}

fn missed_text(check: &Check, output: &str, date: &str) -> String {
    let issue = check.owner.text();
    format!("Check {} missed on {date}, so {issue} is open again: its box \"{}\" carries the output. `{}` gave {output}. You deferred that box; fix what it measures or re-defer it.", check.id, check.text, check.command)
}

fn settle(filing: &mut Filing, checks: &[Check], check: &Check, ran: &Ran, date: &str) -> Result<String, String> {
    let (body, open) = filing.issues.body(&check.owner)?;
    let issue = check.owner.text();
    if body.contains(&settled(check, "passed")) || body.contains(&settled(check, "missed")) {
        filing.filed.record_key(key(check))?;
        return Ok(format!("{}: already settled on {issue}", check.id));
    }
    let edited = settled_body(&body, check, ran, date)?;
    filing.issues.set_body(&check.owner, &edited)?;
    let told = match ran {
        Ran::Passed(_) if open && boxes::unticked(&edited).next().is_none() => filing.issues.close(&check.owner).map(|()| String::new())?,
        Ran::Missed(output) => reopen(filing, checks, check, open, &missed_text(check, output, date))?,
        _ => String::new(),
    };
    filing.filed.record_key(key(check))?;
    Ok(format!("{}: {} on {issue}{told}", check.id, if matches!(ran, Ran::Passed(_)) { "ticked" } else { "reopened" }))
}

/// Reopens the issue of a missed check, so the judge closes it again once its boxes land, and tells the lead that deferred the box.
fn reopen(filing: &mut Filing, checks: &[Check], check: &Check, open: bool, text: &str) -> Result<String, String> {
    if !open {
        filing.issues.reopen(&check.owner)?;
    }
    for sibling in checks.iter().filter(|sibling| sibling.owner == check.owner) {
        filing.filed.forget_key(&closed(sibling))?;
    }
    let lead = check.lead.as_deref().unwrap_or(NO_LEAD);
    Ok(match filing.leads.tell(lead, text) {
        Ok(()) => format!(", told {lead}"),
        Err(why) => format!(", not told {lead}: {why}"),
    })
}

enum Closing {
    Open,
    WasClosed,
    Closed,
}

fn closing(filing: &Filing, checks: &[Check], owner: &Issue) -> Result<Closing, String> {
    let (body, open) = filing.issues.body(owner)?;
    if !open {
        return Ok(Closing::WasClosed);
    }
    if !box_oracles::landed(&body, |id| checks.iter().any(|check| check.id == id)) {
        return Ok(Closing::Open);
    }
    filing.issues.close(owner).map(|()| Closing::Closed)
}

/// Closes each open issue that still waits on a check not yet due once its work has landed: every unticked box waits
/// on a check this registry holds and every ticked box is proven.
fn close_landed(checks: &[Check], today: Day, filing: &mut Filing) -> (Vec<String>, bool) {
    let waiting: Vec<&Check> = checks.iter().filter(|check| today < check.review && !filing.filed.has_key(&closed(check))).collect();
    let mut owners: Vec<&Issue> = Vec::new();
    for check in &waiting {
        if !owners.contains(&&check.owner) {
            owners.push(&check.owner);
        }
    }
    let (mut lines, mut failed) = (Vec::new(), false);
    for owner in owners {
        let issue = owner.text();
        let recorded = closing(filing, checks, owner).and_then(|state| {
            match state {
                Closing::Open => return Ok(()),
                Closing::Closed => lines.push(format!("{issue}: closed, since its work landed and its open boxes wait on dated checks")),
                Closing::WasClosed => {}
            }
            waiting.iter().filter(|check| check.owner == *owner).try_for_each(|check| filing.filed.record_key(closed(check)))
        });
        if let Err(why) = recorded {
            failed = true;
            lines.push(format!("{issue}: not closed: {why}"));
        }
    }
    (lines, failed)
}

/// Runs every due check not yet filed and, when `filing` is given, ticks or reopens its box, then closes each issue
/// whose work has landed. A check that could not run is reported and tried again on the next tick.
pub fn settle_due(checks: &[Check], today: Day, root: &Path, runner: &dyn Fn(&Check, &Path) -> Ran, mut filing: Option<Filing>) -> (Vec<String>, bool) {
    let mut lines = Vec::new();
    let mut failed = false;
    let date = today.text();
    for check in checks.iter().filter(|check| today >= check.review) {
        if filing.as_ref().is_some_and(|filing| filing.filed.has_key(&key(check))) {
            continue;
        }
        let ran = runner(check, root);
        let read = match &ran {
            Ran::Passed(output) => format!("{}: pass ({output})", check.id),
            Ran::Missed(output) => format!("{}: miss ({output})", check.id),
            Ran::Unrun(why) => {
                lines.push(format!("{}: not run: {why}; tried again on the next tick", check.id));
                continue;
            }
        };
        lines.push(read);
        if let Some(filing) = filing.as_mut() {
            match settle(filing, checks, check, &ran, &date) {
                Ok(done) => lines.push(done),
                Err(why) => {
                    failed = true;
                    lines.push(format!("{}: not filed: {why}", check.id));
                }
            }
        }
    }
    if let Some(filing) = filing.as_mut() {
        let (closed, close_failed) = close_landed(checks, today, filing);
        lines.extend(closed);
        failed |= close_failed;
    }
    (lines, failed)
}

pub const DEFER_USAGE: &str = "usage: north check experiments defer ISSUE BOX --check CMD --by YYYY-MM-DD [--id ID] [--expect TEXT] [--lead SESSION] [--root DIR]";

struct Deferral {
    check: Check,
    root: std::path::PathBuf,
}

fn issue_of(text: &str) -> Option<Issue> {
    Issue::parse(&if text.contains('/') { text.to_owned() } else { format!("tompassarelli/{text}") })
}

fn slug(text: &str) -> String {
    let words: Vec<String> = text
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(5)
        .map(str::to_ascii_lowercase)
        .collect();
    words.join("-")
}

fn deferral(arguments: &[String], today: Day, mut lead: Option<String>) -> Result<Deferral, String> {
    let mut positional = Vec::new();
    let (mut command, mut by, mut id, mut expect, mut root) = (None, None, None, None, None);
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        let mut value = |flag: &str| words.next().cloned().ok_or_else(|| format!("{flag} needs a value"));
        match word.as_str() {
            "--check" => command = Some(value("--check")?),
            "--by" => by = Some(value("--by")?),
            "--id" => id = Some(value("--id")?),
            "--expect" => expect = Some(value("--expect")?),
            "--lead" => lead = Some(value("--lead")?),
            "--root" => root = Some(std::path::PathBuf::from(value("--root")?)),
            flag if flag.starts_with("--") => return Err(format!("unknown flag {flag}")),
            _ => positional.push(word.clone()),
        }
    }
    let [issue, text] = positional.as_slice() else { return Err("name the issue and the box".to_owned()) };
    let owner = issue_of(issue).ok_or_else(|| format!("{issue} is not owner/repo#N or repo#N"))?;
    let by = by.ok_or("--by names the review day")?;
    let review = Day::parse(&by).ok_or_else(|| format!("--by {by} is not YYYY-MM-DD"))?;
    if review < today {
        return Err(format!("--by {by} is before today, {}", today.text()));
    }
    let command = command.ok_or("--check names the command the loop runs")?;
    let id = id.unwrap_or_else(|| format!("n{}-{}", owner.number, slug(text)));
    let root = match root {
        Some(root) => root,
        None => std::env::current_dir().map_err(|why| format!("current directory: {why}"))?,
    };
    Ok(Deferral { check: Check { id, text: text.clone(), command, expect, review, owner, lead }, root })
}

fn entry_text(check: &Check) -> String {
    let quoted = |text: &str| serde_json::to_string(text).unwrap_or_default();
    let expect = check.expect.as_deref().map_or_else(String::new, |expect| format!("expect = {}\n", quoted(expect)));
    let lead = check.lead.as_deref().map_or_else(String::new, |lead| format!("lead = {}\n", quoted(lead)));
    format!("\n\n[[check]]\nid = {}\nbox = {}\ncommand = {}\n{expect}review = {}\nowner = {}\n{lead}", quoted(&check.id), quoted(&check.text), quoted(&check.command), quoted(&check.review.text()), quoted(&check.owner.text()))
}

/// `north check experiments defer`: writes the entry into the registry under `--root` (the lane that will
/// land it), naming the lead told on a miss, and marks the box deferred on its issue. The judge closes the issue once
/// that entry is on main and only deferred boxes remain open.
pub fn defer(arguments: &[String], today: Day, lead: Option<String>, issues: &dyn Issues) -> Result<String, String> {
    let Deferral { check, root } = deferral(arguments, today, lead)?;
    let path = root.join(registry::PATH);
    let before = std::fs::read_to_string(&path).map_err(|why| format!("{}: {why}; run it from the north checkout or pass --root", path.display()))?;
    let after = format!("{}{}", before.trim_end(), entry_text(&check));
    let registered = registry::parse(&after).map_err(|problems| problems.join("; "))?.checks;
    let (body, open) = issues.body(&check.owner)?;
    let edited = deferred_body(&body, &check)?;
    std::fs::write(&path, &after).map_err(|why| format!("{}: {why}", path.display()))?;
    if let Err(why) = issues.set_body(&check.owner, &edited) {
        std::fs::write(&path, &before).map_err(|undo| format!("{why}; and restoring {}: {undo}", path.display()))?;
        return Err(why);
    }
    let issue = check.owner.text();
    let rest = if !open {
        format!("{issue} is closed already")
    } else if box_oracles::landed(&edited, |id| registered.iter().any(|entry| entry.id == id)) {
        format!("{issue} closes on the judge's first run after the lane lands, and reopens if the check misses")
    } else {
        format!("{issue} stays open for its other boxes and closes once they land")
    };
    Ok(format!("deferred the box of {issue} to {} check {}, judged on {}; {rest}; land {} with the lane", registry::PATH, check.id, check.review.text(), registry::PATH))
}

#[cfg(test)]
mod tests;
