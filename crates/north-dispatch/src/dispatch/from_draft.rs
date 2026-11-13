//! A launched box's draft (north#1233): a passing draft lands on the box's launch once its check passes again on
//! current main and the landing checks pass there; a stale, failed or check-blind draft goes into the launched
//! worker's brief as text, so the worker starts from it rather than from nothing.
use super::drafts::{self, Draft, Outcome};
use super::plan::Launch;
use super::survey::Survey;
use super::{box_words, drafting, facets, record};
use crate::pacing::placement::{self, gate};
use crate::platform::process::run_within;
use std::process::Output;
use std::time::Duration;

const LAND_WAIT: Duration = Duration::from_secs(1200);
const SHOW_WAIT: Duration = Duration::from_secs(30);
const HELD_S: f64 = 3600.0;
const DIFF_LINES: usize = 400;

pub(super) enum FromDraft {
    Landed(String),
    Held(String),
    Note(String),
}

#[derive(Debug, PartialEq)]
pub(super) enum Plan {
    Land,
    Held,
    Note(String),
    Fresh,
}

pub(super) fn plan(draft: &Draft, now: f64) -> Plan {
    match draft.outcome {
        Outcome::Landed if now - draft.at < HELD_S => Plan::Held,
        Outcome::Landed | Outcome::Running | Outcome::Abandoned => Plan::Fresh,
        Outcome::DraftPass if draft.commit.is_some() => Plan::Land,
        Outcome::DraftPass => Plan::Note("its finish call recorded no commit".to_owned()),
        Outcome::DraftFail => Plan::Note(format!("its check failed: {}", draft.failure.as_deref().unwrap_or("no output kept"))),
        Outcome::PassesOnBase => Plan::Note("its check passed without its change too, so that check does not show the box".to_owned()),
    }
}

pub(super) fn at_launch(survey: &Survey, launch: &Launch, dry_run: bool) -> Option<FromDraft> {
    let text = facets::declared_in(&launch.brief, "Box")?;
    let sha = drafting::box_sha256(text);
    let kept = drafts::read(&record::directory());
    let item = launch.item.to_string();
    let draft = kept.iter().rev().find(|draft| draft.item == item && draft.box_sha256 == sha)?;
    let draft = drafting::settle(&survey.cfg, &survey.here, draft, None);
    let named = format!("{item} box `{}`", box_words(text));
    match plan(&draft, placement::now()) {
        Plan::Fresh => None,
        Plan::Held => Some(FromDraft::Held(format!("skip {named}: it landed from a candidate, {}, within the hour and waits for its tick", draft.name))),
        Plan::Note(why) => Some(FromDraft::Note(note(survey, &draft, &why))),
        Plan::Land if dry_run => Some(FromDraft::Landed(format!("would land {named} from a candidate, {} on {}, after its check runs again on current main", draft.name, draft.host))),
        Plan::Land => Some(land(survey, &draft, &named, text)),
    }
}

fn land(survey: &Survey, draft: &Draft, named: &str, text: &str) -> FromDraft {
    let lane = draft.lane.to_string_lossy().into_owned();
    let words = ["lane", "land-draft", lane.as_str(), "--item", draft.item.as_str(), "--box", text.trim()];
    let output = north_on(survey, &draft.host, &words, LAND_WAIT);
    let landed = output.as_ref().filter(|output| output.status.success()).and_then(|output| String::from_utf8_lossy(&output.stdout).lines().find_map(|line| line.strip_prefix("landed ").map(str::to_owned)));
    match landed {
        Some(sha) => {
            let state = Draft { at: placement::now(), outcome: Outcome::Landed, commit: Some(sha.clone()), ..draft.clone() };
            if let Err(why) = drafts::append(&record::directory(), &state) {
                println!("draft {}: landed, its state unrecorded: {why}", draft.name);
            }
            FromDraft::Landed(format!("landed {named} from a candidate, {} on {}: {sha}", draft.name, draft.host))
        }
        None => {
            let why = output.map_or_else(|| format!("north lane land-draft did not return within {} s", LAND_WAIT.as_secs()), |output| tail(&String::from_utf8_lossy(&output.stderr), 20));
            FromDraft::Note(note(survey, draft, &format!("it did not land on current main: {why}")))
        }
    }
}

fn note(survey: &Survey, draft: &Draft, why: &str) -> String {
    let lane = draft.lane.to_string_lossy().into_owned();
    let words: Vec<&str> = match draft.commit {
        Some(_) => vec!["git", "-C", lane.as_str(), "show", "--format=", "HEAD"],
        None => vec!["git", "-C", lane.as_str(), "diff", draft.base.as_str()],
    };
    let diff = on_host(survey, &draft.host, &words, SHOW_WAIT).filter(|output| output.status.success()).map(|output| String::from_utf8_lossy(&output.stdout).into_owned());
    let change = match diff.filter(|diff| !diff.trim().is_empty()) {
        Some(diff) => format!("Its change, to reuse or discard:\n```diff\n{}\n```", head(&diff, DIFF_LINES)),
        None => format!("Its change could not be read from {} on {}.", lane, draft.host),
    };
    format!(
        "\n\nCandidate (north#1233): draft {} computed this box on the small model on base {} with check `{}`, and {why}. {change}",
        draft.name, draft.base, draft.check
    )
}

fn head(text: &str, lines: usize) -> String {
    let kept: Vec<&str> = text.lines().take(lines).collect();
    let more = text.lines().count().saturating_sub(lines);
    if more == 0 { kept.join("\n") } else { format!("{}\n... {more} more lines", kept.join("\n")) }
}

fn tail(text: &str, lines: usize) -> String {
    let all: Vec<&str> = text.lines().filter(|line| !line.trim().is_empty()).collect();
    all[all.len().saturating_sub(lines)..].join("\n")
}

fn north_on(survey: &Survey, host: &str, words: &[&str], wait: Duration) -> Option<Output> {
    let north = if host == survey.here { std::env::current_exe().ok()?.to_string_lossy().into_owned() } else { "north".to_owned() };
    on_host(survey, host, &std::iter::once(north.as_str()).chain(words.iter().copied()).collect::<Vec<_>>(), wait)
}

fn on_host(survey: &Survey, host: &str, words: &[&str], wait: Duration) -> Option<Output> {
    let (program, rest) = words.split_first()?;
    if host == survey.here {
        return run_within(program, rest, wait);
    }
    let ssh = gate::machines(&survey.cfg).into_iter().find(|machine| machine.name == host)?.ssh;
    let command = words.iter().map(|word| format!("'{}'", word.replace('\'', r"'\''"))).collect::<Vec<_>>().join(" ");
    run_within("ssh", &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", &ssh, &command], wait)
}
