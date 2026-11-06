//! A session whose last turn ended on its account's usage limit parks on `usage:ACCOUNT@TIME`;
//! `north fleet waits` revives it once the reset has passed or a reading of that window newer than
//! the park shows room.
use std::sync::LazyLock;
use std::time::{Duration, SystemTime};

use regex::Regex;

use super::model::{Ending, EntryKind, SessionRecord, Status, UsageLimit};
use super::registry::Fleet;
use super::transcript::stream_path;
use super::waits::{Registered, Run};
use crate::usage::decide::world::{Label, Reading, Unix, Window};

const RECENT: Duration = Duration::from_secs(48 * 3600);
const UNREADABLE_RETRY_MS: u64 = 3_600_000;
const DAY_MS: u64 = 86_400_000;
const LABEL: &str = "usage limit";
const FIVE_HOURS_MS: u64 = 5 * 3_600_000;
pub const FULL_PCT: f64 = 99.5;

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

#[derive(Clone, Debug, PartialEq)]
pub struct UsageWait {
    pub account: Label,
    pub window: Window,
    pub until: Unix,
    pub parked_at: Unix,
}

impl UsageWait {
    pub fn of(wait: &Registered) -> Option<Self> {
        let Run::Usage { account, at_ms } = &wait.run else { return None };
        let window = if at_ms.saturating_sub(wait.since_ms) > FIVE_HOURS_MS { Window::Week } else { Window::FiveHour };
        Some(Self { account: Label(account.clone()), window, until: Unix((at_ms / 1000) as i64), parked_at: Unix((wait.since_ms / 1000) as i64) })
    }

    pub fn newer_reading<'a>(&self, readings: &'a [Reading]) -> Option<&'a Reading> {
        readings.iter().filter(|reading| reading.account == self.account && reading.window == self.window && reading.read_at > self.parked_at).max_by_key(|reading| reading.read_at)
    }

    pub fn lifted(&self, readings: &[Reading], now_ms: u64) -> Option<String> {
        if now_ms >= self.until.0 as u64 * 1000 {
            return Some("reset".to_owned());
        }
        self.newer_reading(readings).filter(|reading| reading.used.0 < FULL_PCT).map(|reading| format!("reads {:.0}% used", reading.used.0))
    }
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
    use std::cell::RefCell;
    use std::path::Path;

    use super::*;
    use crate::fleet::waits::{Probe, Waits};

    struct Clock(u64);

    impl Probe for Clock {
        fn result(&self, wait: &Registered) -> Option<String> {
            UsageWait::of(wait)?.lifted(&[], self.0)
        }
    }

    #[test]
    fn a_session_parked_on_its_accounts_usage_limit_is_resumed_by_the_tick_at_the_reset_and_not_before() {
        let home = tempfile::tempdir().unwrap();
        let waits = Waits::in_root(home.path());
        let reset_ms = 1_793_934_000_000;
        waits.register("worker", Run::Usage { account: "c".into(), at_ms: reset_ms }, Path::new("/tmp"), Some(LABEL)).unwrap();
        let told = RefCell::new(Vec::new());
        let tell = |name: &str, text: &str| -> crate::error::NorthResult<()> {
            told.borrow_mut().push((name.to_owned(), text.to_owned()));
            Ok(())
        };

        let early = waits.settle(&Clock(reset_ms - 60_000), &|_| true, &tell);
        let held = waits.of("worker").len();
        let due = waits.settle(&Clock(reset_ms), &|_| true, &tell);

        assert!(early.is_empty() && held == 1, "{early:?}");
        assert_eq!(due.len(), 1);
        assert_eq!(told.into_inner(), [("worker".to_owned(), resume_message("c"))]);
        assert!(waits.of("worker").is_empty(), "the wait is released once the session is told");
    }
}
