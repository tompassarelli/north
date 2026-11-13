//! `north proxy move [--to ACCOUNT] [--effort LEVEL] [--auto]`: the coordinator follows the account with the most week headroom before its own
//! account runs out. A move is a revive of the `proxy` record onto the account, which carries the conversation and keeps the model; the
//! host holds the session, so the move looks at no window and runs the same from a sandboxed shell.
use std::path::PathBuf;
use std::time::Duration;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::Effort;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::PROXY;
use crate::foundation::clock::Timestamp;
use crate::usage::outlook::{AccountOutlook, Runway};

use super::placement::Overrides;
use super::{revive, router};

const COOLDOWN: Duration = Duration::from_secs(20 * 60);
const DECIDE_EVERY: Duration = Duration::from_secs(5 * 60);
const MOVED: &str = "proxy-move.stamp";
const DECIDED: &str = "proxy-move.decided";
pub(super) const USAGE: &str = "usage: north proxy move [--to ACCOUNT] [--effort low|medium|high|xhigh|max] [--auto]";

pub(super) fn run(words: &[&str]) -> NorthResult<()> {
    let (mut to, mut effort, mut auto) = (None, None, false);
    let mut rest = words.iter();
    while let Some(word) = rest.next() {
        match *word {
            "--to" => to = Some(rest.next().ok_or_else(|| NorthError::Usage(USAGE.into()))?.to_string()),
            "--effort" => effort = Some(Effort::parse(rest.next().ok_or_else(|| NorthError::Usage(USAGE.into()))?)?),
            "--auto" => auto = true,
            _ => return Err(NorthError::Usage(USAGE.into())),
        }
    }
    let fleet = router::open_fleet()?;
    if auto {
        return start_when_due(&fleet);
    }
    let record = fleet.fleet_proxy()?.ok_or_else(|| NorthError::Refused("the fleet records no coordinator".into()))?;
    match decide(&record.account, &outlooks()?, to.as_deref()) {
        Verdict::Stay { reason } => println!("north proxy move: staying on account {}: {reason}", record.account),
        Verdict::Stranded { reason } => return Err(NorthError::Refused(format!("the coordinator on account {} cannot move: {reason}", record.account))),
        Verdict::Move { to, reason } => move_to(&fleet, &record.account, &to, &reason, effort)?,
    }
    Ok(())
}

pub(super) fn auto(fleet: &Fleet) {
    if let Err(error) = start_when_due(fleet) {
        eprintln!("north proxy move: {}", error.user_message());
    }
}

fn start_when_due(fleet: &Fleet) -> NorthResult<()> {
    if !elapsed(MOVED, COOLDOWN) || !elapsed(DECIDED, DECIDE_EVERY) {
        return Ok(());
    }
    stamp(DECIDED);
    let Some(record) = fleet.fleet_proxy()? else { return Ok(()) };
    let Verdict::Move { to, reason } = decide(&record.account, &outlooks()?, None) else { return Ok(()) };
    move_to(fleet, &record.account, &to, &reason, None)
}

fn move_to(fleet: &Fleet, from: &str, to: &str, reason: &str, effort: Option<Effort>) -> NorthResult<()> {
    println!("north proxy move: account {from} to {to}: {reason}");
    stamp(MOVED);
    revive::restart_onto(fleet, PROXY, None, effort, Overrides::default(), Some(to))?;
    println!("north proxy move: moved {PROXY} to account {to}");
    Ok(())
}

fn outlooks() -> NorthResult<Vec<AccountOutlook>> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| NorthError::Usage("no home directory".into()))?;
    let db = crate::usage::open_ledger(&home, None, &crate::threads::client::refresh_mirrored_snapshot).map_err(NorthError::State)?;
    crate::usage::outlook::claude_accounts(&db, &crate::platform::orchestration::path(), Timestamp::now()).map_err(NorthError::State)
}

fn stamp_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state/north").join(name))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

fn elapsed(name: &str, interval: Duration) -> bool {
    let Some(stamp) = stamp_path(name) else { return false };
    let last = std::fs::read_to_string(&stamp).ok().and_then(|text| text.trim().parse::<u64>().ok()).unwrap_or(0);
    now_secs().saturating_sub(last) >= interval.as_secs()
}

fn stamp(name: &str) {
    if let Some(stamp) = stamp_path(name) {
        let _ = std::fs::write(stamp, now_secs().to_string());
    }
}

pub(super) const TRIGGER_MINUTES: f64 = 45.0;

#[derive(Debug, PartialEq)]
pub(super) enum Verdict {
    Stay { reason: String },
    Move { to: String, reason: String },
    Stranded { reason: String },
}

pub(super) fn endangered(outlook: &AccountOutlook) -> bool {
    match outlook.runway {
        Runway::Out => true,
        Runway::Empties { minutes } => minutes < TRIGGER_MINUTES,
        Runway::Lasts | Runway::Unknown => false,
    }
}

fn headroom(outlook: &AccountOutlook) -> f64 {
    100.0 - outlook.week_used.unwrap_or(100.0)
}

fn fit_target(outlook: &AccountOutlook) -> bool {
    outlook.week_used.is_some_and(|used| used < 100.0) && !endangered(outlook)
}

pub(super) fn decide(current: &str, outlooks: &[AccountOutlook], requested: Option<&str>) -> Verdict {
    let Some(mine) = outlooks.iter().find(|outlook| outlook.account == current) else {
        return Verdict::Stranded { reason: format!("account {current} has no usage reading") };
    };
    if let Some(to) = requested {
        return match outlooks.iter().find(|outlook| outlook.account == to) {
            _ if to == current => Verdict::Stay { reason: format!("the coordinator already runs on account {current}") },
            Some(target) if fit_target(target) => Verdict::Move { to: to.to_owned(), reason: format!("asked to move to account {to} with {:.0}% of its week left", headroom(target)) },
            Some(_) => Verdict::Stranded { reason: format!("account {to} is out or burning toward its limit") },
            None => Verdict::Stranded { reason: format!("account {to} has no usage reading") },
        };
    }
    if !endangered(mine) {
        return Verdict::Stay { reason: format!("account {current} lasts past {TRIGGER_MINUTES:.0} minutes") };
    }
    let best = outlooks.iter().filter(|outlook| outlook.account != current && fit_target(outlook)).max_by(|left, right| headroom(left).total_cmp(&headroom(right)).then_with(|| right.account.cmp(&left.account)));
    match best {
        Some(target) => Verdict::Move { to: target.account.clone(), reason: format!("account {current} runs out in under {TRIGGER_MINUTES:.0} minutes; account {} has the most week headroom at {:.0}%", target.account, headroom(target)) },
        None => Verdict::Stranded { reason: format!("account {current} runs out in under {TRIGGER_MINUTES:.0} minutes and no other account has week headroom") },
    }
}
