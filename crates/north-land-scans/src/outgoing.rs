use std::path::{Path, PathBuf};
use std::time::Duration;

use north_base::foundation::exit::NO_VERDICT;
use north_base::platform::private_text::ASKING_STATUS;

use crate::platform::betterleaks;
use crate::landing::step::{Refused, Step, git_read};
use crate::platform::process::{exit_status, on_path};

pub mod denylist;
pub mod email;
pub mod language;
pub mod native_ref;
pub mod secrets;

use self::denylist::{Denylist, Stand};
use self::email::commit_email_scan;
use self::native_ref::native_ref_scan;

const BETTERLEAKS_MISSING: &str = "safe-push: betterleaks not on PATH — refusing to publish the land check branch without a secret scan; NOT pushed.";
const SECRET_FOUND: &str = "safe-push: ✗ SECRET DETECTED — fix the leak; NOT pushed.";
pub const JUDGING: u8 = ASKING_STATUS as u8;

pub fn secrets(dir: &Path, range: &str) -> Step {
    exit_status(betterleaks::command("betterleaks").current_dir(dir).args(betterleaks::history_args(range)).status())
}

fn origin_url(dir: &Path) -> Step<String> {
    git_read(dir, &["remote", "get-url", "--push", "origin"]).map_err(|_| Refused(2))
}

fn top_level(dir: &Path) -> Step<PathBuf> {
    git_read(dir, &["rev-parse", "--show-toplevel"]).map(PathBuf::from).map_err(|_| Refused(2))
}

pub fn denylist(dir: &Path, base: &str, head: &str, reference: &str) -> Step {
    let origin = origin_url(dir)?;
    Denylist::from_env().scan(&top_level(dir)?, &origin, base, head, reference)
}

pub fn publication(dir: &Path, base: &str, head: &str) -> Step {
    let origin = origin_url(dir)?;
    if !on_path("betterleaks") {
        eprintln!("{BETTERLEAKS_MISSING}");
        return Err(Refused(1));
    }
    let range = if base.is_empty() { head.to_owned() } else { format!("{base}..{head}") };
    if secrets(dir, &range).is_err() {
        eprintln!("{SECRET_FOUND}");
        return Err(Refused(1));
    }
    Denylist::from_env().scan(&top_level(dir)?, &origin, base, head, "main")?;
    commit_email_scan(dir, &[range])
}

/// Whether the enqueue's scans judged the lane, lacked only the private-kind verdict the lander must ask for again, or left the private-kind judge asking a model, whose verdict the lander waits for before it lands the lane.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Gate {
    Judged,
    NoVerdict,
    Judging,
}

pub fn enqueue_secrets(root: &Path, (base, head): (&str, &str)) -> Result<(), String> {
    if on_path("betterleaks") {
        return secrets(root, &format!("{base}..{head}")).map_err(|_| "secret scan refused the landing".to_owned());
    }
    println!("safe-push: ⚠ betterleaks is not on PATH here, so this lane was not scanned for secrets before queueing; the lander's scan stays the gate.");
    Ok(())
}

pub fn enqueue_gate(root: &Path, (base, head): (&str, &str), denylist: &Denylist, origin: &str, destination: &str) -> Result<Gate, String> {
    match denylist.stand_at_once(root, origin, base, head, destination).map_err(|_| "deny-list scan refused the landing".to_owned())? {
        Stand::Pass => Ok(Gate::Judged),
        Stand::NoVerdict => Ok(Gate::NoVerdict),
        Stand::Judging => Ok(Gate::Judging),
        Stand::Refuse => unreachable!("stand_at_once returns a refusal as Err"),
    }
}

fn private_kinds(dir: &Path, (base, head, reference): (&str, &str, &str), wait: Option<&str>) -> Step {
    let wait = match wait.map(str::parse::<u64>) {
        None => None,
        Some(Ok(millis)) => Some(Duration::from_millis(millis)),
        Some(Err(_)) => return Err(Refused(2)),
    };
    let verdict = Denylist::from_env().verdict_within(&top_level(dir)?, &origin_url(dir)?, (base, head, reference), wait);
    verdict.lines.iter().for_each(|line| println!("{line}"));
    match verdict.stand {
        Stand::Pass => Ok(()),
        Stand::Refuse => Err(Refused(1)),
        Stand::Judging => Err(Refused(JUDGING)),
        Stand::NoVerdict => Err(Refused(NO_VERDICT)),
    }
}

struct NativeRefs {
    ranges: Vec<String>,
    names: Vec<String>,
}

fn native_refs(args: &[String]) -> Option<NativeRefs> {
    let mut parsed = NativeRefs { ranges: Vec::new(), names: Vec::new() };
    let mut rest = args.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--range" => parsed.ranges.push(rest.next()?.clone()),
            "--" => {
                parsed.names.extend(rest.cloned());
                break;
            }
            _ => return None,
        }
    }
    Some(parsed)
}

fn dispatch(dir: &Path, command: &str, rest: &[String]) -> Option<Step> {
    let rest: Vec<&str> = rest.iter().map(String::as_str).collect();
    Some(match (command, rest.as_slice()) {
        ("denylist", [base, head, reference]) => denylist(dir, base, head, reference),
        ("private-kinds", [base, head, reference]) => private_kinds(dir, (base, head, reference), None),
        ("private-kinds", [base, head, reference, wait]) => private_kinds(dir, (base, head, reference), Some(wait)),
        ("commit-email", revisions) => commit_email_scan(dir, &revisions.iter().map(|revision| (*revision).to_owned()).collect::<Vec<_>>()),
        ("secrets", [range]) => secrets(dir, range),
        ("publication", [base, head]) => publication(dir, base, head),
        _ => return None,
    })
}

pub fn run(arguments: &[String]) -> Option<u8> {
    let (command, rest) = arguments.split_first()?;
    let dir = std::env::current_dir().ok()?;
    let outcome = if command == "native-ref" {
        native_refs(rest).map(|parsed| {
            let names: Vec<&str> = parsed.names.iter().map(String::as_str).collect();
            native_ref_scan(&dir, &names, &parsed.ranges)
        })
    } else {
        dispatch(&dir, command, rest)
    };
    outcome.map(|step| match step {
        Ok(()) => 0,
        Err(Refused(code)) => code,
    })
}

pub fn instruction_limits(root: &Path, reference: &str) -> Step {
    match crate::landing::instruction_limits::check(root, reference) {
        0 => Ok(()),
        code => Err(Refused(code)),
    }
}
