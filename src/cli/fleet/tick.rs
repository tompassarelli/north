use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::{NorthError, NorthResult};
use crate::dispatch::{IssueRef, work_gone};
use crate::fleet::launch_queue::QueuedLaunch;
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;
use crate::fleet::stage::detach;
use crate::fleet::store::OverSsh;
use crate::fleet::tick::{Budgets, FarHost, Pulls, Step, Ticked, open_ledger, probe_pass, tick};
use crate::pacing::placement::{config, gate, pass};

use super::api_spend;
use super::dispatch_tick;
use super::ledger::Ledger;
use super::rebooted;
use super::waits::Probes;
use super::window_move;

const USAGE: &str = "Usage: north fleet tick";
const BESIDE: Duration = Duration::from_secs(20);
pub(super) const QUEUE_ENTRY: &str = "NORTH_FLEET_QUEUE_ENTRY";
const QUEUE_RETRY_MS: u64 = 3 * 60 * 1000;
const QUEUE_BUDGET: Duration = Duration::from_secs(15);

fn always_pulled(fleet: &Fleet) -> Vec<FarHost> {
    config().map(|cfg| gate::spool_pulls(&cfg, fleet.host())).unwrap_or_default().into_iter().map(|(name, ssh)| FarHost { name, ssh }).collect()
}

fn dispatch_pass(fleet: &Fleet) -> dispatch_tick::Pass {
    let cfg = config().ok();
    let paused = crate::dispatch::paused();
    dispatch_tick::pass(fleet.host(), cfg.as_ref(), paused.as_deref(), &crate::dispatch::launch_directory(), now_ms() as f64 / 1000.0, || crate::pacing::placement::resident_clis(fleet.host(), ""), || crate::dispatch::run(&[], super::router::roster))
}

fn dispatch_line(dispatched: &dispatch_tick::Pass) -> Option<String> {
    match dispatched {
        dispatch_tick::Pass::Ran(code) if *code != 0 => Some(format!("dispatch exited {code}")),
        _ => None,
    }
}

pub(super) fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    if !words.is_empty() {
        return Err(NorthError::Usage(USAGE.into()));
    }
    let ledger = open_ledger(|| Ok(Ledger));
    let probes = Probes::here(fleet);
    if probes == Probes::Registry {
        let ticked = probe_pass(fleet, &ledger, &probes, Budgets::TICK)?;
        ticked.lines().iter().for_each(|line| println!("{line}"));
        drain_queue(fleet)?.iter().for_each(|line| println!("{line}"));
        println!("{}", ticked.timing());
        if let Some(line) = dispatch_line(&dispatch_pass(fleet)) {
            println!("{line}");
        }
        return Ok(());
    }
    let began = Instant::now();
    let spending = {
        let fleet = fleet.clone();
        detach("api_spend", move || (api_spend::pull(&fleet), Instant::now()))
    };
    let moving = detach("window_move", || {
        window_move::auto();
        Instant::now()
    });
    let _pass = pass();
    let pulls = Pulls { spools: std::sync::Arc::new(OverSsh::default()), always: always_pulled(fleet) };
    let ticked = tick(fleet, &ledger, &pulls, &probes, now_ms(), Budgets::TICK);
    let reviving = Instant::now();
    let revived = rebooted::revive_within(fleet, rebooted::REVIVE_BUDGET.min(Budgets::TICK.worst().saturating_sub(began.elapsed())));
    let mut ticked = ticked?;
    ticked.steps.push(Step { name: "rebooted", began: reviving - began, took: reviving.elapsed() });
    let deadline = (began + BESIDE).max(Instant::now());
    let spend_lines = match spending.by(deadline) {
        Ok((lines, ended)) => {
            ticked.steps.push(Step { name: "api_spend", began: Duration::ZERO, took: ended - began });
            lines
        }
        Err(late) => late_beside(&mut ticked, "api_spend", &late.why(BESIDE), began),
    };
    let moved = match moving.by(deadline) {
        Ok(ended) => {
            ticked.steps.push(Step { name: "window_move", began: Duration::ZERO, took: ended - began });
            Vec::new()
        }
        Err(late) => late_beside(&mut ticked, "window_move", &late.why(BESIDE), began),
    };
    let queueing = Instant::now();
    let queued = drain_queue(fleet)?;
    ticked.steps.push(Step { name: "launch_queue", began: queueing - began, took: queueing.elapsed() });
    let dispatching = Instant::now();
    let dispatched = dispatch_pass(fleet);
    ticked.steps.push(Step { name: "dispatch", began: dispatching - began, took: dispatching.elapsed() });
    ticked.wall = began.elapsed();
    ticked.lines().iter().chain(&spend_lines).chain(&moved).chain(&queued).for_each(|line| println!("{line}"));
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

fn drain_queue(fleet: &Fleet) -> NorthResult<Vec<String>> {
    let started = Instant::now();
    let now = crate::fleet::runtime::now_ms();
    let due = |queued: &QueuedLaunch| queued.tried_ms.is_none_or(|tried| now.saturating_sub(tried) >= QUEUE_RETRY_MS);
    let mut lines = fleet.drop_unstartable(&due, &finished)?;
    for queued in fleet.queued_launches()? {
        if !due(&queued) {
            continue;
        }
        fleet.mark_queued_tried(&queued.name, now)?;
        let north = std::env::current_exe()?;
        let spawned = Command::new(north)
            .args(["fleet", "launch"])
            .args(&queued.words)
            .env(QUEUE_ENTRY, &queued.name)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        match spawned {
            Ok(_) => lines.push(format!("started queued launch {} at {} priority", queued.name, queued.priority.label())),
            Err(error) => lines.push(format!("cannot start queued launch {}: {error}", queued.name)),
        }
        if started.elapsed() >= QUEUE_BUDGET {
            break;
        }
    }
    Ok(lines)
}

fn late_beside(ticked: &mut Ticked, name: &'static str, why: &str, began: Instant) -> Vec<String> {
    ticked.steps.push(Step { name, began: Duration::ZERO, took: began.elapsed() });
    vec![format!("deferred {name} to the next tick: {why}")]
}
