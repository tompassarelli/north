use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use north_runtime_follow::LANDER_ROLLBACK;

use super::after::{follow, send_condition};

pub const CLASS: &str = "lander-held";
const FILE: &str = "lander-held";
const UNREACHED: &str = "lander-unreached";
const ALARM_AFTER_SECONDS: u64 = 15 * 60;
pub const RETRY_AFTER_QUIET_SECONDS: u64 = 15 * 60;
pub const CLOCK: &str = "NORTH_LAND_PROMOTE_CLOCK";
const PROXY: &str = "proxy";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hold {
    pub since: u64,
}

fn parse(text: &str) -> Option<Hold> {
    let mut words = text.lines().next()?.split_whitespace();
    Some(Hold { since: words.next()?.parse().ok()? })
}

fn render(hold: Hold) -> String {
    format!("{}\n", hold.since)
}

pub fn now() -> SystemTime {
    std::env::var(CLOCK).ok().and_then(|seconds| seconds.parse().ok()).map_or_else(SystemTime::now, |seconds| UNIX_EPOCH + Duration::from_secs(seconds))
}

pub fn now_seconds() -> u64 {
    now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// The hold a refused promotion extends and whether it has now lasted past the alarm line; the fleet store pages once per hold, keyed by its start.
pub fn extend(previous: Option<Hold>, now: u64) -> (Hold, bool) {
    let hold = previous.unwrap_or(Hold { since: now });
    (hold, now.saturating_sub(hold.since) >= ALARM_AFTER_SECONDS)
}

/// Whether a runtime held off the lander since `held_since` is due its retry: the lander landed nothing, and the hold has stood, for `RETRY_AFTER_QUIET_SECONDS` before `now`.
pub fn quiet(held_since: SystemTime, landed: impl IntoIterator<Item = SystemTime>, now: SystemTime) -> bool {
    let last = landed.into_iter().fold(held_since, SystemTime::max);
    now.duration_since(last).is_ok_and(|quiet| quiet.as_secs() >= RETRY_AFTER_QUIET_SECONDS)
}

fn path(home: &Path) -> PathBuf {
    home.join(FILE)
}

pub fn since(home: &Path) -> Option<u64> {
    fs::read_to_string(path(home)).ok().as_deref().and_then(parse).map(|hold| hold.since)
}

pub fn reason(home: &Path) -> Option<String> {
    fs::read_to_string(path(home)).ok()?.lines().nth(1).map(str::trim).filter(|reason| !reason.is_empty()).map(str::to_owned)
}

pub fn clear(home: &Path) {
    let _ = fs::remove_file(path(home));
    let _ = fs::remove_file(home.join(UNREACHED));
    release_rollback(home);
}

/// Names the runtime the lander link leaves for a promotion's candidate, so runtime collection keeps it while the candidate is judged and a rollback has somewhere to go.
pub fn record_rollback(home: &Path, previous: &Path) {
    let _ = fs::write(home.join(LANDER_ROLLBACK), format!("{}\n", previous.display()));
}

pub fn release_rollback(home: &Path) {
    let _ = fs::remove_file(home.join(LANDER_ROLLBACK));
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
        .args(["ledger", "incident", "record", "--key", &key(hold), "--class", CLASS, "--noticed-by", super::incidents::LANDER, "--subject", candidate, "--detail", reason])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Notes one refused promotion in `home`; past the alarm line it records the hold's one incident row and messages the proxy under the hold's condition key, which the fleet store delivers once per hold.
pub fn held(home: &Path, now: u64, candidate: &str, reason: &str) {
    let previous = fs::read_to_string(path(home)).ok().as_deref().and_then(parse);
    let (hold, due) = extend(previous, now);
    if due {
        let minutes = now.saturating_sub(hold.since) / 60;
        let recorded = record(hold, candidate, reason);
        let told = send_condition(PROXY, &key(hold), false, &message(minutes, candidate, reason));
        if !recorded || !told {
            println!("north land promote: the {minutes}-minute hold alarm reached {}", if recorded || told { "only part of its targets" } else { "none of its targets" });
        }
    }
    let _ = fs::create_dir_all(home);
    let _ = fs::write(path(home), format!("{}{}\n", render(hold), reason.replace('\n', " ")));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_alarms_from_fifteen_minutes_under_one_key_and_its_file_round_trips() {
        let (first, due) = extend(None, 1000);
        assert_eq!((first, due), (Hold { since: 1000 }, false));
        let (soon, due) = extend(Some(first), 1000 + 14 * 60);
        assert!(!due);
        let (late, due) = extend(Some(soon), 1000 + 15 * 60);
        let (after, still) = extend(Some(late), 1000 + 40 * 60);
        assert!(due && still && key(late) == key(after), "every alarm of one hold names one condition, which the fleet store delivers once");

        assert_eq!(parse(&render(Hold { since: 42 })), Some(Hold { since: 42 }));
    }
}
