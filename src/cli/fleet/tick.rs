use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::{NorthError, NorthResult};
use crate::foundation::clock::Timestamp;
use crate::dispatch::{IssueRef, work_gone};
use crate::fleet::launch_queue::{Admission, QueuedLaunch};
use crate::fleet::model::Delivery;
use crate::fleet::sender::Sender;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;
use crate::fleet::stage::{Detached, detach};
use crate::fleet::store::OverSsh;
use crate::fleet::tick::{Budgets, Cpu, FarHost, Pulls, Step, Ticked, detach_metered, metered, open_ledger, probe_pass, tick};
use crate::pacing::placement::{config, gate, pass};
use crate::platform::shared_config;

use super::api_spend;
use super::dispatch_tick;
use super::ledger::Ledger;
use super::rebooted;
use super::waits::Probes;
use super::proxy_move;

const USAGE: &str = "Usage: north fleet tick";
const BESIDE: Duration = Duration::from_secs(20);
pub(super) const QUEUE_ENTRY: &str = "NORTH_FLEET_QUEUE_ENTRY";
const QUEUE_RETRY_MS: u64 = 3 * 60 * 1000;
type Refresh = (Option<Result<shared_config::Stamp, String>>, Instant, Duration);

fn always_pulled(fleet: &Fleet) -> Vec<FarHost> {
    config().map(|cfg| gate::spool_pulls(&cfg, fleet.host())).unwrap_or_default().into_iter().map(|(name, ssh)| FarHost { name, ssh }).collect()
}

fn dispatch_pass(fleet: &Fleet) -> dispatch_tick::Pass {
    let cfg = config().ok();
    let paused = crate::dispatch::review_pause();
    dispatch_tick::pass(fleet.host(), cfg.as_ref(), paused.as_deref(), &crate::dispatch::launch_directory(), now_ms() as f64 / 1000.0, || crate::pacing::placement::resident_clis(fleet.host(), ""), || crate::dispatch::run(&[], super::router::roster))
}

fn dispatch_beside(fleet: &Fleet, began: Instant) -> (Result<dispatch_tick::Pass, String>, Option<Duration>) {
    let fleet = fleet.clone();
    match dispatch_tick::within(Budgets::TICK.worst().saturating_sub(began.elapsed()), move || metered(|| dispatch_pass(&fleet))) {
        Ok((dispatched, cpu)) => (Ok(dispatched), Some(cpu)),
        Err(deferred) => (Err(deferred), None),
    }
}

fn dispatch_line(dispatched: &Result<dispatch_tick::Pass, String>) -> Option<String> {
    match dispatched {
        Ok(dispatch_tick::Pass::Ran(code)) if *code != 0 => Some(format!("dispatch exited {code}")),
        Err(deferred) => Some(deferred.clone()),
        _ => None,
    }
}

pub(super) fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    if !words.is_empty() {
        return Err(NorthError::Usage(USAGE.into()));
    }
    let (began, spent) = (Instant::now(), Cpu::process());
    let refreshing = detach("shared_config", || {
        let (stamp, cpu) = metered(|| shared_config::refresh_if_due(&home(), shared_config::now()));
        (stamp, Instant::now(), cpu)
    });
    let ledger = open_ledger(|| Ok(Ledger));
    let probes = Probes::here(fleet);
    if probes == Probes::Registry {
        let ticked = probe_pass(fleet, &ledger, &probes, Budgets::TICK)?;
        ticked.lines().iter().for_each(|line| println!("{line}"));
        drain_queue(fleet, began)?.0.iter().for_each(|line| println!("{line}"));
        kept_fresh(fleet, &refreshing, began).0.iter().for_each(|line| println!("{line}"));
        println!("{}", ticked.timing());
        if let Some(line) = dispatch_line(&dispatch_beside(fleet, began).0) {
            println!("{line}");
        }
        return Ok(());
    }
    let spending = {
        let fleet = fleet.clone();
        detach("api_spend", move || {
            let (lines, cpu) = metered(|| api_spend::pull(&fleet));
            (lines, Instant::now(), cpu)
        })
    };
    let moving = {
        let fleet = fleet.clone();
        detach("proxy_move", move || {
            let ((), cpu) = metered(|| proxy_move::auto(&fleet));
            (Instant::now(), cpu)
        })
    };
    let _pass = pass();
    let pulls = Pulls { spools: std::sync::Arc::new(OverSsh::default()), always: always_pulled(fleet) };
    let ticked = tick(fleet, &ledger, &pulls, &probes, now_ms(), Budgets::TICK);
    let reviving = Instant::now();
    let (revived, reviving_cpu) = metered(|| rebooted::revive_within(fleet, rebooted::REVIVE_BUDGET.min(Budgets::TICK.worst().saturating_sub(began.elapsed()))));
    let mut ticked = ticked?;
    ticked.steps.push(Step { name: "rebooted", began: reviving - began, took: reviving.elapsed(), cpu: Some(reviving_cpu) });
    let deadline = (began + BESIDE).max(Instant::now());
    let spend_lines = match spending.by(deadline) {
        Ok((lines, ended, cpu)) => {
            ticked.steps.push(Step { name: "api_spend", began: Duration::ZERO, took: ended - began, cpu: Some(cpu) });
            lines
        }
        Err(late) => late_beside(&mut ticked, "api_spend", &late.why(BESIDE), began),
    };
    let moved = match moving.by(deadline) {
        Ok((ended, cpu)) => {
            ticked.steps.push(Step { name: "proxy_move", began: Duration::ZERO, took: ended - began, cpu: Some(cpu) });
            Vec::new()
        }
        Err(late) => late_beside(&mut ticked, "proxy_move", &late.why(BESIDE), began),
    };
    let queueing = Instant::now();
    let (queued, queueing_cpu) = drain_queue(fleet, began)?;
    ticked.steps.push(Step { name: "launch_queue", began: queueing - began, took: queueing.elapsed(), cpu: queueing_cpu });
    let expiring = Instant::now();
    let (expired, expiring_cpu) = metered(|| crate::cli::machines::expire(Timestamp::now()));
    ticked.steps.push(Step { name: "expire", began: expiring - began, took: expiring.elapsed(), cpu: Some(expiring_cpu) });
    let handing = Instant::now();
    let (handed, handing_cpu) = metered(|| super::committed_lanes::pass(fleet));
    ticked.steps.push(Step { name: "committed_lanes", began: handing - began, took: handing.elapsed(), cpu: Some(handing_cpu) });
    let (refreshed, ended, refreshing_cpu) = kept_fresh(fleet, &refreshing, began);
    ticked.steps.push(Step { name: "shared_config", began: Duration::ZERO, took: ended - began, cpu: refreshing_cpu });
    let dispatching = Instant::now();
    let (dispatched, dispatching_cpu) = dispatch_beside(fleet, began);
    ticked.steps.push(Step { name: "dispatch", began: dispatching - began, took: dispatching.elapsed(), cpu: dispatching_cpu });
    ticked.wall = began.elapsed();
    ticked.cpu = spent.spent();
    ticked.lines().iter().chain(&spend_lines).chain(&moved).chain(&queued).chain(&expired).chain(&handed).chain(&refreshed).for_each(|line| println!("{line}"));
    println!("{}", ticked.timing());
    if let Some(line) = dispatch_line(&dispatched) {
        println!("{line}");
    }
    revived
}

/// Why the issue or box a queued launch was for no longer needs a worker, by GitHub's own copy of it.
fn finished(queued: &QueuedLaunch) -> Option<String> {
    let item = IssueRef::parse(&queued.request.task()?)?;
    work_gone(&item, queued.request.claimed_box().as_deref())
}

fn drain_queue(fleet: &Fleet, began: Instant) -> NorthResult<(Vec<String>, Option<Duration>)> {
    let left = Budgets::TICK.worst().saturating_sub(began.elapsed());
    let budget = Budgets::TICK.queue_after(began.elapsed());
    let admitting = {
        let fleet = fleet.clone();
        detach_metered("launch_queue", move || {
            let (started, now) = (Instant::now(), now_ms());
            let due = |queued: &QueuedLaunch| queued.tried_ms.is_none_or(|tried| now.saturating_sub(tried) >= QUEUE_RETRY_MS);
            fleet.admit_queued(now, &Admission { due: &due, gone: &finished, start: &start, budget, spent: &|| started.elapsed() })
        })
    };
    match admitting.by(Instant::now() + left) {
        Ok((lines, cpu)) => lines.map(|lines| (lines, Some(cpu))),
        Err(late) => Ok((vec![format!("deferred launch_queue to the next tick: {}", late.why(left))], None)),
    }
}

fn start(queued: &QueuedLaunch) -> Result<(), String> {
    let north = std::env::current_exe().map_err(|error| error.to_string())?;
    let spawned = Command::new(north).args(["fleet", "launch"]).args(&queued.words).env(QUEUE_ENTRY, &queued.name).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    spawned.map(drop).map_err(|error| error.to_string())
}

fn late_beside(ticked: &mut Ticked, name: &'static str, why: &str, began: Instant) -> Vec<String> {
    ticked.steps.push(Step { name, began: Duration::ZERO, took: began.elapsed(), cpu: None });
    vec![format!("deferred {name} to the next tick: {why}")]
}

fn home() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// The shared-config refresh started beside the tick, waited for only until its deadline, then the coordinator's line once its snapshot is past three quarters of the bound, which goes out whether the fetch failed, ran late or never ran.
fn kept_fresh(fleet: &Fleet, refreshing: &Detached<Refresh>, began: Instant) -> (Vec<String>, Instant, Option<Duration>) {
    let mut lines = Vec::new();
    let (ended, cpu) = match refreshing.by((began + BESIDE).max(Instant::now())) {
        Ok((refreshed, ended, cpu)) => {
            if let Some(Err(error)) = refreshed {
                lines.push(format!("shared_config: {error}"));
            }
            (ended, Some(cpu))
        }
        Err(late) => {
            lines.push(format!("shared_config: the refresh is {}; the next tick fetches again", late.why(BESIDE)));
            (Instant::now(), None)
        }
    };
    let (home, now) = (home(), shared_config::now());
    if let Some(stamp) = shared_config::overdue(&home, now) {
        let line = format!("{}: {}", fleet.host(), shared_config::overdue_line(&stamp, now));
        match fleet.send_signed(&Sender::fleet(), "proxy", &line, Delivery::Queued) {
            Ok(()) => {
                if let Err(error) = shared_config::warned(&home, &stamp) {
                    lines.push(format!("shared_config: told the coordinator but could not record it: {error}"));
                }
            }
            Err(error) => lines.push(format!("shared_config: could not tell the coordinator its snapshot is late: {error}")),
        }
        lines.push(line);
    }
    (lines, ended, cpu)
}
