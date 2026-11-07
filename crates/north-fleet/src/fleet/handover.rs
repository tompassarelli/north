//! A session is taken over onto another source when `usage::decide::assess` says so: a worker with
//! unlanded work once its source is out, a proxy or lead before its source runs out. The new session
//! gets a `Follows:` brief and the old one's lane, and a lead goes first.
use std::collections::{BTreeMap, BTreeSet};

use north_base::platform::attribution::{self, Cause, Harness};

use super::limit::{UsageWait, park, recent, unknown_reset_ms};
use super::model::{Ending, LaunchRequest, Role, SessionRecord};
use super::registry::Fleet;
use super::waits::Run;
use crate::error::NorthResult;
use crate::foundation::clock::Timestamp;
use crate::usage::decide::world::{
    Account, Credit, EstimateSource, Expected, Host, Label, LimitEvent, ModelFamily, Name, Owner, Provider, Rate, Rates, Reading, Session, Settings, Standing, Unix, Usd, World,
};
use crate::usage::decide::{Action, Assessments, assess};

/// What the usage table says about every account, read outside the fleet registry.
#[derive(Default)]
pub struct Observed {
    pub readings: Vec<Reading>,
    pub limits: Vec<LimitEvent>,
    pub credits: BTreeMap<String, Credit>,
}

pub trait AccountReadings {
    fn observed(&self, now_ms: u64) -> Observed;
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct HandOver {
    pub lines: Vec<String>,
    pub alert: Option<String>,
}

pub fn hand_over(
    fleet: &Fleet,
    readings: &dyn AccountReadings,
    holds_work: &dyn Fn(&SessionRecord) -> bool,
    take_over: &dyn Fn(&LaunchRequest) -> NorthResult<SessionRecord>,
    tell_tom: &dyn Fn(&str) -> Result<(), String>,
    now_ms: u64,
) -> HandOver {
    let mut done = HandOver::default();
    let records = candidates(fleet);
    if records.is_empty() {
        let _ = fleet.set_all_out_announced(false);
        return done;
    }
    let host = Host(fleet.host().to_owned());
    let observed = readings.observed(now_ms);
    let accounts: Vec<Account> = fleet.accounts().into_iter().filter(|account| account.provider.label() == "claude").filter_map(|account| account_of(account, &observed.credits, &host)).collect();
    let waits = fleet.waits();
    let now = Unix((now_ms / 1000) as i64);
    let mut limits = observed.limits;
    let leads: BTreeSet<&str> = records.iter().filter_map(|record| record.parent.as_deref()).collect();
    let sessions: Vec<Session> = records
        .iter()
        .map(|record| {
            let parked = waits.of(&record.name).iter().find_map(UsageWait::of);
            if let Some(wait) = parked.as_ref().filter(|wait| wait.newer_reading(&observed.readings).is_none()) {
                limits.push(LimitEvent { account: wait.account.clone(), window: wait.window, resets_at: Some(wait.until), at: wait.parked_at });
            }
            let standing = if record.role() == Role::Proxy { Standing::Proxy } else if leads.contains(record.name.as_str()) { Standing::Lead } else { Standing::Worker };
            session_of(record, standing, parked.map(|wait| wait.parked_at))
        })
        .collect();
    let settings = Settings::default();
    let world = World { now, accounts: &accounts, readings: &observed.readings, limits: &limits, sessions: &sessions, settings: &settings };
    let previous = Assessments { all_out: fleet.all_out_announced().then_some(crate::usage::decide::AllOut { earliest_resume: None, announce: false }), ..Assessments::default() };
    let assessed = assess(&world, &previous);
    match &assessed.all_out {
        Some(out) if out.announce => {
            let line = all_out_line(out.earliest_resume, now);
            match tell_tom(&line).map(|()| fleet.set_all_out_announced(true)) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => done.lines.push(format!("the all-out line reached Tom but was not recorded, so the next tick sends it again: {}", error.user_message())),
                Err(why) => done.lines.push(format!("the all-out line did not reach Tom, so the next tick sends it again: {why}")),
            }
            done.alert = Some(line);
        }
        Some(_) => {}
        None => {
            let _ = fleet.set_all_out_announced(false);
        }
    }
    for (name, action) in assessed.actions {
        let Some(record) = records.iter().find(|record| record.name == name.0) else { continue };
        let to = match action {
            Action::HandOver { to } => to,
            Action::Park { resumes_at } => {
                let until_ms = resumes_at.and_then(|at| u64::try_from(at.0).ok()).map(|at| at * 1000).filter(|at| *at > now_ms).unwrap_or_else(|| unknown_reset_ms(now_ms));
                match park(&waits, record, until_ms) {
                    Ok(parked) => done.lines.push(format!("{} (account {} is out)", parked.line(), record.account)),
                    Err(error) => done.lines.push(format!("{}: account {} is paused but its wait was not registered: {}", record.name, record.account, error.user_message())),
                }
                continue;
            }
        };
        let proxy = record.role() == Role::Proxy;
        if !proxy && !holds_work(record) {
            continue;
        }
        attribution::record(&record.name, "fleet", Cause::Harness(Harness::Capacity), &format!("handed over from account {} to {}", record.account, to.0));
        let request = LaunchRequest {
            account: to.0.clone(),
            prompt: if proxy { proxy_brief(record, &to.0) } else { brief(record, &to.0) },
            placement: Some(format!("handed over from account {}", record.account)),
            resume: proxy.then(|| record.conversation.clone()).flatten(),
            ..record.relaunch_request()
        };
        match take_over(&request) {
            Ok(_) => {
                let _ = waits.clear_usage(&record.name);
                done.lines.push(format!("{}: handed over from account {} to {} with its lane {}", record.name, record.account, to.0, record.cwd.display()));
            }
            Err(error) => done.lines.push(format!("{}: not handed over from account {} to {}: {}", record.name, record.account, to.0, error.user_message())),
        }
    }
    done
}

fn candidates(fleet: &Fleet) -> Vec<SessionRecord> {
    let waits = fleet.waits();
    let parked = |name: &str| waits.of(name).iter().any(|wait| matches!(wait.run, Run::Usage { .. }));
    fleet
        .sessions()
        .unwrap_or_default()
        .into_iter()
        .filter(|record| record.provider.label() == "claude" && !record.verification)
        .filter(|record| !matches!(record.ended, Some(Ending::Stopped | Ending::IdleExit)))
        .filter(|record| recent(record) && (fleet.running(record) || parked(&record.name)))
        .collect()
}

fn account_of(account: crate::fleet::model::Account, credits: &BTreeMap<String, Credit>, host: &Host) -> Option<Account> {
    let credit = match &account.api {
        Some(key) if account.label.contains(':') => Some(*credits.get(&key.org)?),
        Some(_) => return None,
        None => None,
    };
    let rate = Rate::Pooled { points_per_usd: 1.0, n: 0 };
    Some(Account {
        provider: Provider::Claude,
        label: Label(account.label),
        weight: 1.0,
        signed_in: BTreeSet::from([host.clone()]),
        extra_usage_enabled: Some(false),
        extra_usage_spend: None,
        rates: Rates { five_hour: rate, week: rate },
        credit,
    })
}

fn session_of(record: &SessionRecord, standing: Standing, parked_since: Option<Unix>) -> Session {
    Session {
        name: Name(record.name.clone()),
        owner: Owner::Fleet,
        account: Label(record.account.clone()),
        family: family_of(record.model.as_deref()),
        workstream: None,
        box_id: None,
        standing,
        spent: None,
        active_minutes: 0.0,
        expected: Expected { p50: Usd(0.0), p90: Usd(0.0), minutes: 0.0, source: EstimateSource::None },
        parked_since,
    }
}

fn family_of(model: Option<&str>) -> ModelFamily {
    let model = model.unwrap_or_default().to_ascii_lowercase();
    [("fable", ModelFamily::Fable), ("sonnet", ModelFamily::Sonnet), ("haiku", ModelFamily::Haiku)].into_iter().find(|(word, _)| model.contains(word)).map_or(ModelFamily::Opus, |(_, family)| family)
}

fn brief(record: &SessionRecord, to: &str) -> String {
    let follows = record.task_ref.clone().unwrap_or_else(|| record.name.clone());
    format!(
        "Follows: {follows} (handed over from account {from} to {to}, which is above the floor)\n\n\
         [north: account {from} is at or under its floor, so `north fleet tick` took over {name} onto account {to}] Your lane at {cwd} is intact and your earlier conversation ended on {from}. \
         Run `git fetch origin`, then check whether your lane is on origin/main. \
         If it is not, rebase it onto origin/main, re-run the targeted checks and re-enqueue it with `safe-push --to main`. \
         If it is, report `Done:` with the landed sha.",
        from = record.account,
        name = record.name,
        cwd = record.cwd.display(),
    )
}

fn proxy_brief(record: &SessionRecord, to: &str) -> String {
    format!(
        "[north: account {from} is at or under its floor, so `north fleet tick` moved {name} onto account {to}, the source above the floor with the most headroom] This is the same conversation; carry on where it stopped.",
        from = record.account,
        name = record.name,
    )
}

fn all_out_line(earliest: Option<Unix>, now: Unix) -> String {
    let when = earliest.map_or_else(
        || "at a time no account has reported".to_owned(),
        |at| {
            let clock = Timestamp::from_unix_seconds(at.0).local_twelve_hour();
            let days = (at.0 - now.0).max(0) / 86_400;
            if days == 0 { format!("at {clock}") } else { format!("at {clock} in {days} day{}", if days == 1 { "" } else { "s" }) }
        },
    );
    format!("Every Claude account is out of usage for the sessions in flight; the earliest resumes {when}, and they wait until then.")
}

