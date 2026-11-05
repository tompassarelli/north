//! A session whose last turn ended on its account's usage limit parks on `usage:ACCOUNT@TIME`;
//! `north fleet waits` revives it once the reset has passed.
use std::sync::LazyLock;
use std::time::{Duration, SystemTime};

use regex::Regex;

use super::model::{Ending, EntryKind, SessionRecord, Status, UsageLimit};
use super::registry::Fleet;
use super::transcript::stream_path;
use super::waits::Run;

const RECENT: Duration = Duration::from_secs(48 * 3600);
const UNREADABLE_RETRY_MS: u64 = 3_600_000;
const DAY_MS: u64 = 86_400_000;
const LABEL: &str = "usage limit";

static CLOCK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\bresets\s+(\d{1,2})(?::(\d{2}))?\s*(am|pm)\s*\(utc\)").expect("limit clock pattern"));

/// The reset of an account's usage window as `agents usage` reports it, for a limit whose turn named none.
pub trait AccountResets {
    fn reset_ms(&self, account: &str, now_ms: u64) -> Option<u64>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Parked {
    pub session: String,
    pub account: String,
    pub until_ms: u64,
}

impl Parked {
    pub fn line(&self) -> String {
        format!("{}: parked on {}", self.session, Run::Usage { account: self.account.clone(), at_ms: self.until_ms })
    }
}

/// When the limit lifts: the reset the stream carried, else the clock time its message names in UTC,
/// else the account's reset, else one retry hour from now.
pub fn lifts_at_ms(limit: &UsageLimit, now_ms: u64, account: impl FnOnce() -> Option<u64>) -> u64 {
    let stream = limit.resets_at_s.map(|seconds| seconds.saturating_mul(1000));
    let future = |at: Option<u64>| at.filter(|at| *at > now_ms);
    let lifts = future(stream).or_else(|| future(limit.message.as_deref().and_then(|text| clock_after(text, now_ms)))).or_else(|| future(account()));
    lifts.unwrap_or(now_ms + UNREADABLE_RETRY_MS).div_ceil(1000) * 1000
}

fn clock_after(text: &str, now_ms: u64) -> Option<u64> {
    let found = CLOCK.captures(text)?;
    let (hour, minute): (u64, u64) = (found[1].parse().ok()?, found.get(2).map_or(Some(0), |minute| minute.as_str().parse().ok())?);
    if !(1..=12).contains(&hour) || minute > 59 {
        return None;
    }
    let pm = found[3].eq_ignore_ascii_case("pm");
    let in_day = ((hour % 12) + if pm { 12 } else { 0 }) * 3_600_000 + minute * 60_000;
    let today = now_ms - now_ms % DAY_MS + in_day;
    Some(if today > now_ms { today } else { today + DAY_MS })
}

/// Registers a `usage:` wait for each session whose last turn ended on the account's limit and that
/// holds none yet; a session `north fleet stop` ended is left alone.
pub fn park_limited(fleet: &Fleet, resets: &dyn AccountResets, now_ms: u64) -> Vec<Parked> {
    let waits = fleet.waits();
    let mut parked = Vec::new();
    for record in fleet.sessions().unwrap_or_default() {
        if record.ended == Some(Ending::Stopped) || !recent(&record) || waits.of(&record.name).iter().any(|wait| matches!(wait.run, Run::Usage { .. })) {
            continue;
        }
        let Ok(follower) = fleet.follow_recent(&record.name) else { continue };
        let transcript = follower.transcript();
        let ended = transcript.status != Status::Working && transcript.entries.last().is_some_and(|entry| entry.kind == EntryKind::TurnEnd);
        let Some(limit) = transcript.limit.as_ref().filter(|_| ended) else { continue };
        let until_ms = lifts_at_ms(limit, now_ms, || resets.reset_ms(&record.account, now_ms));
        let run = Run::Usage { account: record.account.clone(), at_ms: until_ms };
        match waits.register(&record.name, run, &record.cwd, Some(LABEL)) {
            Ok(()) => parked.push(Parked { session: record.name.clone(), account: record.account.clone(), until_ms }),
            Err(error) => eprintln!("north fleet: {} hit its usage limit but its wait was not registered: {}", record.name, error.user_message()),
        }
    }
    parked
}

pub(super) fn recent(record: &SessionRecord) -> bool {
    std::fs::metadata(stream_path(&record.host_dir)).and_then(|stream| stream.modified()).is_ok_and(|at| SystemTime::now().duration_since(at).is_ok_and(|age| age < RECENT))
}

pub fn resume_message(account: &str) -> String {
    format!(
        "[north: account {account}'s usage limit has reset] Your last turn stopped on the limit. Run `git fetch origin`, then check whether your lane is on origin/main. \
         If it is not, rebase it onto origin/main, re-run the targeted checks and re-enqueue it with `safe-push --to main`. \
         If it is, report `Done:` with the landed sha."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOON_MS: u64 = 1_793_000_000_000 - 1_793_000_000_000 % DAY_MS + 12 * 3_600_000;

    fn limit(resets_at_s: Option<u64>, message: Option<&str>) -> UsageLimit {
        UsageLimit { resets_at_s, message: message.map(str::to_owned) }
    }

    #[test]
    fn the_stream_reset_wins_then_the_message_clock_then_the_account_then_an_hour() {
        let account = || Some(NOON_MS + 7_000);
        assert_eq!(lifts_at_ms(&limit(Some(NOON_MS / 1000 + 60), Some("You've hit your limit · resets 11:50pm (UTC)")), NOON_MS, account), NOON_MS + 60_000);
        assert_eq!(lifts_at_ms(&limit(None, Some("You've hit your session limit · resets 11:50pm (UTC)")), NOON_MS, account), NOON_MS + 11 * 3_600_000 + 50 * 60_000);
        assert_eq!(lifts_at_ms(&limit(None, Some("You've hit your limit · resets 3pm")), NOON_MS, account), NOON_MS + 7_000);
        assert_eq!(lifts_at_ms(&limit(None, None), NOON_MS, || None), NOON_MS + UNREADABLE_RETRY_MS);
    }

    #[test]
    fn a_stale_reset_is_passed_over_and_a_clock_already_past_today_means_tomorrow() {
        assert_eq!(lifts_at_ms(&limit(Some(NOON_MS / 1000 - 5), None), NOON_MS, || Some(NOON_MS + 9_000)), NOON_MS + 9_000);
        assert_eq!(lifts_at_ms(&limit(None, Some("resets 11am (UTC)")), NOON_MS, || None), NOON_MS + 23 * 3_600_000);
        assert_eq!(clock_after("resets 13pm (UTC)", NOON_MS), None);
        assert_eq!(clock_after("resets 12am (UTC)", NOON_MS), Some(NOON_MS + 12 * 3_600_000));
    }
}
