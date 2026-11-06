use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::after::{follow, send};

pub const CLASS: &str = "lander-held";
const FILE: &str = "lander-held";
const UNREACHED: &str = "lander-unreached";
const ALARM_AFTER_SECONDS: u64 = 15 * 60;
const ALARMED: &str = "alarmed";
const PROXY: &str = "proxy";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hold {
    pub since: u64,
    pub alarmed: bool,
}

fn parse(text: &str) -> Option<Hold> {
    let mut words = text.split_whitespace();
    let since = words.next()?.parse().ok()?;
    Some(Hold { since, alarmed: words.next() == Some(ALARMED) })
}

fn render(hold: Hold) -> String {
    if hold.alarmed { format!("{} {ALARMED}\n", hold.since) } else { format!("{}\n", hold.since) }
}

/// The hold a refused promotion extends and whether it has now lasted past the alarm line without one.
pub fn extend(previous: Option<Hold>, now: u64) -> (Hold, bool) {
    let hold = previous.unwrap_or(Hold { since: now, alarmed: false });
    let due = !hold.alarmed && now.saturating_sub(hold.since) >= ALARM_AFTER_SECONDS;
    (Hold { alarmed: hold.alarmed || due, ..hold }, due)
}

fn path(home: &Path) -> PathBuf {
    home.join(FILE)
}

pub fn since(home: &Path) -> Option<u64> {
    fs::read_to_string(path(home)).ok().as_deref().and_then(parse).map(|hold| hold.since)
}

pub fn clear(home: &Path) {
    let _ = fs::remove_file(path(home));
    let _ = fs::remove_file(home.join(UNREACHED));
}

/// A promotion whose replays of `candidate` reached no verdict, and how many promotions in a row did so.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unreached {
    pub candidate: String,
    pub attempts: u32,
    pub reason: String,
}

pub fn unreached_of(home: &Path) -> Option<Unreached> {
    let text = fs::read_to_string(home.join(UNREACHED)).ok()?;
    let mut fields = text.trim_end().splitn(3, '\t');
    Some(Unreached { candidate: fields.next()?.to_owned(), attempts: fields.next()?.parse().ok()?, reason: fields.next().unwrap_or_default().to_owned() })
}

/// Notes one promotion of `candidate` whose replays reached no verdict; it writes no hold, so it alarms no one and the next promotion retries.
pub fn unreached(home: &Path, candidate: &str, reason: &str) {
    let attempts = unreached_of(home).filter(|previous| previous.candidate == candidate).map_or(1, |previous| previous.attempts + 1);
    let _ = fs::create_dir_all(home);
    let _ = fs::write(home.join(UNREACHED), format!("{candidate}\t{attempts}\t{}\n", reason.replace(['\t', '\n'], " ")));
}

pub fn key(hold: Hold) -> String {
    format!("{CLASS}:{}", hold.since)
}

pub fn message(minutes: u64, candidate: &str, reason: &str) -> String {
    format!("north land: the lander link has been held for {minutes} minutes: {reason}; the lander still runs an older runtime than {candidate}, so lanes that need the newer runtime are refused and the sheriff's fix cannot deploy itself")
}

fn record(hold: Hold, candidate: &str, reason: &str) -> bool {
    Command::new(follow::program())
        .args(["incident", "record", "--key", &key(hold), "--class", CLASS, "--noticed-by", super::incidents::LANDER, "--subject", candidate, "--detail", reason])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Notes one refused promotion in `home`; past the alarm line, once per hold, records an incident row and messages the proxy.
pub fn held(home: &Path, now: u64, candidate: &str, reason: &str) {
    let previous = fs::read_to_string(path(home)).ok().as_deref().and_then(parse);
    let (hold, due) = extend(previous, now);
    if due {
        let minutes = now.saturating_sub(hold.since) / 60;
        let recorded = record(hold, candidate, reason);
        let told = send(PROXY, &message(minutes, candidate, reason));
        if !recorded || !told {
            println!("north land promote: the {minutes}-minute hold alarm reached {}", if recorded || told { "only part of its targets" } else { "none of its targets" });
        }
    }
    let _ = fs::create_dir_all(home);
    let _ = fs::write(path(home), render(hold));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_alarms_once_after_fifteen_minutes_and_its_file_round_trips() {
        let (first, due) = extend(None, 1000);
        assert_eq!((first, due), (Hold { since: 1000, alarmed: false }, false));
        let (soon, due) = extend(Some(first), 1000 + 14 * 60);
        assert!(!due && !soon.alarmed);
        let (late, due) = extend(Some(soon), 1000 + 15 * 60);
        assert!(due && late.alarmed);
        let (after, due) = extend(Some(late), 1000 + 40 * 60);
        assert!(!due && after.alarmed);

        let hold = Hold { since: 42, alarmed: true };
        assert_eq!(parse(&render(hold)), Some(hold));
        assert_eq!(parse(&render(Hold { since: 7, alarmed: false })), Some(Hold { since: 7, alarmed: false }));
    }
}
