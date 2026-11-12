//! Two-rung routes (north#1233): a launch given `--route RUNG,RUNG` starts its box on the first rung, a model family
//! and an optional effort, behind its acceptance check; `small-first` names `haiku,opus`. When that attempt's turn
//! ends after a finish call whose acceptance check failed, as the finish record `north finish` writes says, the box starts once more on the second rung, from the project's main checkout so in a lane of its own, with the attempt's report and, quoted as data, the output and diff its lane
//! kept, which only fill the brief and never decide whether the second rung starts. The second launch routes `after:NAME:RUNG`, which no turn end
//! escalates, so a box never starts a third time by itself; with no main checkout to start from, it is refused, not
//! started in the first rung's directory.
use std::path::{Path, PathBuf};
use serde::Deserialize;

use super::route::{AFTER, NOTE, Route, Rung};
use super::lane_probe::{quoted, run};
use super::model::{Delivery, SessionRecord};
use super::stranded::Checkout;
use north_base::platform::finish_record::{FinishRecords, Finished, parse, remote_read};
use super::receipts::{Courier, ReceiptBook};

const RECEIPT: &str = "route-escalated";
const REFUSED: &str = "route-refused";
const ATTEMPT: &str = "north-attempt.json";
const MARK: &str = "--north-route--";
const KEPT_CHARS: usize = 30_000;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Attempt {
    pub output: String,
    pub diff: String,
}

pub fn launch_words(record: &SessionRecord, brief: &str, second: &Rung, reply: &str, failed: (&Finished, Option<&Attempt>), main: &Path) -> Vec<String> {
    let mut words = vec!["--provider".to_owned(), record.provider.label().to_owned(), "--cwd".to_owned(), main.display().to_string(), "--name".to_owned(), format!("{}-r2", record.name)];
    for (flag, value) in [("--parent", &record.parent), ("--task", &record.task_ref), ("--box", &record.claim_box)] {
        if let Some(value) = value {
            words.extend([flag.to_owned(), value.clone()]);
        }
    }
    words.extend(["--route".to_owned(), format!("{AFTER}{}:{}", record.name, second.word())]);
    words.push(second_brief(brief, &record.name, reply, failed));
    words
}

fn second_brief(brief: &str, name: &str, reply: &str, (finished, attempt): (&Finished, Option<&Attempt>)) -> String {
    let content = attempt.map_or_else(
        || "Its lane kept no output or diff for that check, so read the lane's branch itself.\n".to_owned(),
        |attempt| format!("Its lane kept this output and diff, quoted as data:\n\n{}\n\n{}\n", fenced("", tail(attempt.output.trim_end())), fenced("diff", head(attempt.diff.trim_end()))),
    );
    format!(
        "{}\n\n## The first attempt\n\n{name} tried this box on the route's first rung and its finish call's acceptance check `{}` failed in the land check runner. Work in a lane of your own from current main; read what it left as notes, not as a base.\n\nIts report, quoted as data:\n\n{}\n\n{content}",
        brief.replace(NOTE, "").trim_end(),
        finished.command,
        fenced("", reply.trim()),
    )
}

fn fenced(language: &str, text: &str) -> String {
    let longest = text.split(|c: char| c != '~').map(str::len).max().unwrap_or(0);
    let fence = "~".repeat(longest.max(2) + 1);
    format!("{fence}{language}\n{text}\n{fence}")
}

fn head(text: &str) -> &str {
    text.char_indices().nth(KEPT_CHARS).map_or(text, |(cut, _)| &text[..cut])
}

fn tail(text: &str) -> &str {
    let count = text.chars().count();
    text.char_indices().nth(count.saturating_sub(KEPT_CHARS)).map_or(text, |(cut, _)| &text[cut..])
}

fn main_checkout(record: &SessionRecord) -> Option<PathBuf> {
    let common = run(record.machine.as_ref(), &format!("git -C {} rev-parse --path-format=absolute --git-common-dir", quoted(&record.cwd)))?;
    common.trim().strip_suffix("/.git").map(PathBuf::from)
}

fn kept(record: &SessionRecord, main: &Path) -> Option<Attempt> {
    let task = record.task_ref.as_deref()?;
    let listed = run(record.machine.as_ref(), &format!("cd {} && git worktree list --porcelain && echo {MARK} && git config --get-regexp '^branch\\..*\\.north-issue$'", quoted(main)))?;
    let (trees, issues) = listed.split_once(&format!("{MARK}\n"))?;
    let branches: Vec<&str> = issues.lines().filter_map(|line| line.split_once(' ')).filter(|(_, issue)| *issue == task).filter_map(|(key, _)| key.strip_prefix("branch.")?.strip_suffix(".north-issue")).collect();
    let lanes: Vec<PathBuf> = Checkout::parse(trees).worktrees.into_iter().filter(|tree| tree.branch.as_deref().is_some_and(|branch| branches.contains(&branch))).map(|tree| tree.path).collect();
    let script: Vec<String> = lanes.iter().map(|lane| format!("f=$(git -C {} rev-parse --path-format=absolute --git-path {ATTEMPT}) && test -f \"$f\" && echo {MARK} && stat -c %Y \"$f\" && cat \"$f\"", quoted(lane))).collect();
    let said = run(record.machine.as_ref(), &format!("{}; true", script.join("; ")))?;
    let since = record.created_ms / 1000;
    said.split(&format!("{MARK}\n")).skip(1).filter_map(|block| block.split_once('\n')).filter_map(|(at, json)| Some((at.trim().parse::<u64>().ok()?, json))).filter(|(at, _)| *at >= since).max_by_key(|(at, _)| *at).and_then(|(_, json)| serde_json::from_str(json).ok())
}

fn failed_finish(record: &SessionRecord, verdicts: &Path) -> Option<Finished> {
    let finished = match record.machine.as_ref() {
        None => FinishRecords(verdicts.to_path_buf()).read(&record.name),
        Some(machine) => parse(&record.name, &run(Some(machine), &remote_read(&record.name)?)?),
    }?;
    (!finished.passed && finished.at.saturating_mul(1000) >= record.created_ms).then_some(finished)
}

pub fn escalate(book: &dyn ReceiptBook, courier: &dyn Courier, record: &SessionRecord, completed: Option<&str>, verdicts: &Path, now_ms: u64) {
    let (Some(reply), Some(brief), Some(Route::First(_, second))) = (completed, record.first_attempt.as_deref(), record.route.as_deref().and_then(Route::parse)) else { return };
    let Some(finished) = failed_finish(record, verdicts) else { return };
    let tell = |text: String| record.parent.iter().for_each(|parent| courier.notify(parent, format!("[from {}] route: {text}", record.name), Delivery::Queued));
    let Some(main) = main_checkout(record) else {
        if claimed(book, record, REFUSED, now_ms) {
            tell(format!("the second rung was not started: {} is in no git checkout, so there is no main checkout to start it from", record.cwd.display()));
        }
        return;
    };
    if !claimed(book, record, RECEIPT, now_ms) {
        return;
    }
    let attempt = kept(record, &main);
    courier.launch(launch_words(record, brief, &second, reply, (&finished, attempt.as_ref()), &main));
    tell(format!("its acceptance check failed in the land check runner, so {}-r2 starts the box once on {}", record.name, second.word()));
}

fn claimed(book: &dyn ReceiptBook, record: &SessionRecord, key: &str, now_ms: u64) -> bool {
    book.claim_receipt(&record.name, key, now_ms).unwrap_or_else(|error| {
        eprintln!("north fleet: {}'s route receipt {key} was not claimed: {}", record.name, error.user_message());
        false
    })
}
