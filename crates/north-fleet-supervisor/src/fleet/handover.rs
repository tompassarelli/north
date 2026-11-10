//! A session is taken over onto another source when `usage::decide::assess` says so: a worker with
//! unlanded work once its source is out or paused, a proxy or lead before its source runs out. A worker
//! resumes its own conversation there; one that never started a conversation gets a `Follows:` brief naming the lane its newest land ticket holds, else its brief file, never its working directory, and a lead goes first.
//! A session on an API organization stays out: `api_pool::move_spent` moves it only on the provider's own low-credit refusal, never on north's credit estimate.
use std::collections::{BTreeMap, BTreeSet};

pub mod proxy;
#[cfg(test)]
mod tests;

use north_base::land_ticket::Lane;
use north_base::platform::attribution::{self, Cause, Harness};

use super::limit::{UsageWait, park, recent, unknown_reset_ms};
use super::model::{LaunchRequest, Phase, Role, SessionRecord};
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
    lanes: &BTreeMap<String, Vec<Lane>>,
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
    let mut moving = BTreeSet::new();
    for (name, action) in assessed.actions {
        let Some(record) = records.iter().find(|record| record.name == name.0) else { continue };
        let (to, family) = match action {
            Action::HandOver { to, family } => (to, family),
            Action::Park { resumes_at } => {
                let until_ms = resumes_at.and_then(|at| u64::try_from(at.0).ok()).map(|at| at * 1000).filter(|at| *at > now_ms).unwrap_or_else(|| unknown_reset_ms(now_ms));
                match park(&waits, record, until_ms) {
                    Ok(parked) => done.lines.push(format!("{} (account {} is out)", parked.line(), record.account)),
                    Err(error) => done.lines.push(format!("{}: account {} is paused but its wait was not registered: {}", record.name, record.account, error.user_message())),
                }
                continue;
            }
        };
        if record.role() == Role::Proxy {
            moving.insert(record.name.as_str());
            done.lines.extend(proxy::advance(fleet, record, &to.0, family, take_over, now_ms));
            continue;
        }
        if record.adopted() || !holds_work(record) {
            continue;
        }
        attribution::record(&record.name, "fleet", Cause::Harness(Harness::Capacity), &format!("handed over from account {} to {}", record.account, to.0));
        let lane = lanes.get(&record.name).and_then(|held| held.iter().max_by_key(|lane| lane.queued_ms()));
        let request = LaunchRequest {
            account: to.0.clone(),
            prompt: record.conversation.as_ref().map_or_else(|| brief(record, lane, &to.0), |_| proxy::resumed(record, &to.0)),
            placement: Some(format!("handed over from account {}", record.account)),
            resume: record.conversation.clone(),
            ..record.relaunch_request()
        };
        match take_over(&request) {
            Ok(_) => {
                let _ = waits.clear_usage(&record.name);
                done.lines.push(format!("{}: handed over from account {} to {} {}", record.name, record.account, to.0, lane.map_or_else(|| "with no lane".to_owned(), |lane| format!("with its lane {}", lane.branch))));
            }
            Err(error) => done.lines.push(format!("{}: not handed over from account {} to {}: {}", record.name, record.account, to.0, error.user_message())),
        }
    }
    for record in records.iter().filter(|record| record.role() == Role::Proxy && !moving.contains(record.name.as_str())) {
        proxy::Desk::of(&fleet.root, &record.name).clear();
    }
    done
}

fn candidates(fleet: &Fleet) -> Vec<SessionRecord> {
    let waits = fleet.waits();
    let parked = |name: &str| waits.of(name).iter().any(|wait| matches!(wait.run, Run::Usage { .. }));
    let pools: BTreeSet<String> = fleet.accounts().into_iter().filter(|account| account.api.is_some()).filter_map(|account| account.label.split(':').next().map(str::to_owned)).collect();
    let on_api = |record: &SessionRecord| record.account.split_once(':').is_some_and(|(pool, _)| pools.contains(pool));
    fleet
        .sessions()
        .unwrap_or_default()
        .into_iter()
        .filter(|record| record.provider.label() == "claude" && !record.verification && !on_api(record))
        .filter(|record| !matches!(record.phase(), Some(Phase::Stopped | Phase::IdleExited)))
        .filter(|record| if record.adopted() { fleet.running(record) } else { recent(record) && (fleet.running(record) || parked(&record.name)) })
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

pub(crate) fn family_of(model: Option<&str>) -> ModelFamily {
    let model = model.unwrap_or_default().to_ascii_lowercase();
    [("fable", ModelFamily::Fable), ("sonnet", ModelFamily::Sonnet), ("haiku", ModelFamily::Haiku)].into_iter().find(|(word, _)| model.contains(word)).map_or(ModelFamily::Opus, |(_, family)| family)
}

fn brief(record: &SessionRecord, lane: Option<&Lane>, to: &str) -> String {
    let follows = record.task_ref.clone().unwrap_or_else(|| record.name.clone());
    let from = &record.account;
    let work = match lane {
        Some(lane) => format!(
            "Your lane is branch {branch}, whose newest land ticket is {phase}. \
             Run `git fetch origin`, then check whether {branch} is on origin/main. \
             If it is not, rebase it onto origin/main, re-run the targeted checks and re-enqueue it with `safe-push --to main`. \
             If it is, report `Done:` with the landed sha.",
            branch = lane.branch,
            phase = north_base::lifecycle::State::name(lane.phase),
        ),
        None => {
            let start = record.brief.as_ref().map_or_else(|| format!("Your launch named no brief file, so start from {follows}."), |brief| format!("Your brief is {}; start from it.", brief.display()));
            format!(
                "No land ticket names you, so you have no lane yet: your working directory {cwd} and whatever its HEAD holds are not your work. {start} \
                 Edit in a fresh worktree off origin/main.",
                cwd = record.cwd.display(),
            )
        }
    };
    format!(
        "Follows: {follows} (handed over from account {from} to {to}, which is above the floor)\n\n\
         [north: account {from} is at or under its floor, so `north fleet tick` took over {name} onto account {to}] Your earlier conversation ended on {from}. {work}",
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

