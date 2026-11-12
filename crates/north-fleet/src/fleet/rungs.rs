//! Two-rung routes (north#1233): a launch given `--route RUNG,RUNG` starts its box on the first rung, a model family
//! and an optional effort, behind its acceptance check; `small-first` names `haiku,opus`. When that attempt's turn
//! ends with its last finish call's check failed, a mechanical verdict the finish call wrote (docs/rationale/attribution.md),
//! the box starts once more on the second rung, from the project's main checkout so in a lane of its own, with the attempt's report, its failed check, that
//! check's output and its diff in the brief. The second launch routes `after:NAME:RUNG`, which no turn end
//! escalates, so a box never starts a third time by itself.
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;

use super::effort::Effort;
use super::lane_probe::{quoted, run};
use super::model::{Delivery, Entry, EntryKind, SessionRecord};
use super::receipts::{Courier, ReceiptBook};

pub const SMALL_FIRST: &str = "small-first";
const AFTER: &str = "after:";
const FAMILIES: [&str; 3] = ["haiku", "sonnet", "opus"];
pub const NOTE: &str = "\n\nTwo-rung route: you run on the first rung of a two-rung route. When your finish call's check fails, do not retry: end your turn at once with `Not done:` and `Stop: reasoning`, and when you cannot finish, still make the finish call with the box's check; either way the box starts once on the second rung with your diff and the check's output.";
const RECEIPT: &str = "route-escalated";
const FAILED: &str = "finish: ✗ the check";
const QUEUED: &str = "Done: queued";
const KEPT_CHARS: usize = 30_000;

static KEPT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"the attempt is kept in (/\S+)").expect("kept attempt pattern"));

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rung {
    pub family: &'static str,
    pub effort: Option<Effort>,
}

impl Rung {
    fn parse(text: &str) -> Option<Self> {
        let (family, effort) = match text.split_once(':') {
            Some((family, effort)) => (family, Some(Effort::parse(effort).ok()?)),
            None => (text, None),
        };
        Some(Self { family: FAMILIES.into_iter().find(|known| *known == family)?, effort })
    }

    pub fn word(&self) -> String {
        self.effort.map_or_else(|| self.family.to_owned(), |effort| format!("{}:{}", self.family, effort.label()))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    First(Rung, Rung),
    Second { after: String, rung: Rung },
}

impl Route {
    pub fn parse(text: &str) -> Option<Self> {
        if text == SMALL_FIRST {
            return Some(Self::First(Rung { family: "haiku", effort: None }, Rung { family: "opus", effort: None }));
        }
        if let Some(rest) = text.strip_prefix(AFTER) {
            let (after, rung) = rest.split_once(':').filter(|(after, _)| !after.is_empty())?;
            return Some(Self::Second { after: after.to_owned(), rung: Rung::parse(rung)? });
        }
        let (first, second) = text.split_once(',')?;
        Some(Self::First(Rung::parse(first)?, Rung::parse(second)?))
    }

    pub fn of(brief: &str) -> Option<Self> {
        Self::parse(declared(brief)?)
    }

    pub fn rung(&self) -> &Rung {
        match self {
            Self::First(first, _) => first,
            Self::Second { rung, .. } => rung,
        }
    }
}

pub fn declared(brief: &str) -> Option<&str> {
    brief.lines().find_map(|line| line.trim().strip_prefix("Route:")).map(|value| value.trim().trim_matches('`')).filter(|value| !value.is_empty() && !value.contains(char::is_whitespace))
}

pub fn first(brief: &str) -> bool {
    matches!(Route::of(brief), Some(Route::First(..)))
}

pub fn routed(brief: &str, route: &str) -> String {
    format!("Route: {route}\n{brief}")
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Attempt {
    pub check: String,
    pub output: String,
    pub diff: String,
}

pub fn stopped_short(lines: &[Entry]) -> Option<PathBuf> {
    let last = lines.iter().rev().filter(|entry| entry.kind == EntryKind::ToolResult).find(|entry| entry.text.contains(FAILED) || entry.text.lines().any(|line| line.starts_with(QUEUED)))?;
    KEPT.captures(&last.text).map(|found| PathBuf::from(&found[1]))
}

pub fn launch_words(record: &SessionRecord, brief: &str, second: &Rung, reply: &str, attempt: Option<&Attempt>, main: &Path) -> Vec<String> {
    let mut words = vec!["--provider".to_owned(), record.provider.label().to_owned(), "--cwd".to_owned(), main.display().to_string(), "--name".to_owned(), format!("{}-r2", record.name)];
    for (flag, value) in [("--parent", &record.parent), ("--task", &record.task_ref), ("--box", &record.claim_box)] {
        if let Some(value) = value {
            words.extend([flag.to_owned(), value.clone()]);
        }
    }
    words.extend(["--route".to_owned(), format!("{AFTER}{}:{}", record.name, second.word())]);
    words.push(second_brief(brief, &record.name, reply, attempt));
    words
}

fn second_brief(brief: &str, name: &str, reply: &str, attempt: Option<&Attempt>) -> String {
    let unnoted = brief.replace(NOTE, "");
    let first: Vec<&str> = unnoted.lines().filter(|line| !line.trim_start().starts_with("Route:")).collect();
    let mut text = format!(
        "{}\n\n## The first attempt\n\n{name} tried this box on the route's first rung and stopped short. Work in a lane of your own from current main; read its diff as notes, not as a base.\n\nIts report:\n\n{}\n",
        first.join("\n").trim_end(),
        reply.trim()
    );
    match attempt {
        Some(attempt) => text.push_str(&format!(
            "\nIts check `{}` failed with this output:\n\n~~~\n{}\n~~~\n\nIts diff against origin/main:\n\n~~~diff\n{}\n~~~\n",
            attempt.check,
            tail(attempt.output.trim_end()),
            head(attempt.diff.trim_end())
        )),
        None => text.push_str("\nIt kept no failed check, so there is no check output or diff to read.\n"),
    }
    text
}

fn head(text: &str) -> &str {
    text.char_indices().nth(KEPT_CHARS).map_or(text, |(cut, _)| &text[..cut])
}

fn tail(text: &str) -> &str {
    let count = text.chars().count();
    text.char_indices().nth(count.saturating_sub(KEPT_CHARS)).map_or(text, |(cut, _)| &text[cut..])
}

fn main_checkout(record: &SessionRecord) -> PathBuf {
    let common = run(record.machine.as_ref(), &format!("git -C {} rev-parse --path-format=absolute --git-common-dir", quoted(&record.cwd)));
    common.as_deref().map(str::trim).and_then(|dir| dir.strip_suffix("/.git")).map_or_else(|| record.cwd.clone(), PathBuf::from)
}

fn read(record: &SessionRecord, path: &Path) -> Option<Attempt> {
    serde_json::from_str(&run(record.machine.as_ref(), &format!("cat {}", quoted(path)))?).ok()
}

pub fn escalate(book: &dyn ReceiptBook, courier: &dyn Courier, record: &SessionRecord, reply: &str, lines: &[Entry], now_ms: u64) {
    let Some((brief, path)) = record.first_attempt.as_deref().zip(stopped_short(lines)) else { return };
    let Some(Route::First(_, second)) = Route::of(brief) else { return };
    match book.claim_receipt(&record.name, RECEIPT, now_ms) {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            eprintln!("north fleet: {}'s route escalation was not claimed: {}", record.name, error.user_message());
            return;
        }
    }
    let attempt = read(record, &path);
    courier.launch(launch_words(record, brief, &second, reply, attempt.as_ref(), &main_checkout(record)));
    if let Some(parent) = &record.parent {
        let kept = if attempt.is_some() { "its diff and failed check" } else { "its report" };
        courier.notify(parent, format!("[from {}] route: the attempt stopped short, so {}-r2 starts the box once on {} with {kept}", record.name, record.name, second.word()), Delivery::Queued);
    }
}
