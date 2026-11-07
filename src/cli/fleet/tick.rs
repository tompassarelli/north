use std::time::{Duration, Instant};

use crate::error::{NorthError, NorthResult};
use crate::fleet::registry::Fleet;
use crate::fleet::runtime::now_ms;
use crate::fleet::stage::detach;
use crate::fleet::store::OverSsh;
use crate::fleet::tick::{Budgets, FarHost, Pulls, Step, Ticked, open_ledger, probe_pass, tick};
use crate::pacing::placement::{config, gate, pass};

use super::api_spend;
use super::ledger::Ledger;
use super::rebooted;
use super::waits::Probes;
use super::window_move;

const USAGE: &str = "Usage: north fleet tick";
const BESIDE: Duration = Duration::from_secs(20);

fn always_pulled(fleet: &Fleet) -> Vec<FarHost> {
    config().map(|cfg| gate::spool_pulls(&cfg, fleet.host())).unwrap_or_default().into_iter().map(|(name, ssh)| FarHost { name, ssh }).collect()
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
        println!("{}", ticked.timing());
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
    ticked.wall = began.elapsed();
    ticked.lines().iter().chain(&spend_lines).chain(&moved).for_each(|line| println!("{line}"));
    println!("{}", ticked.timing());
    revived
}

fn late_beside(ticked: &mut Ticked, name: &'static str, why: &str, began: Instant) -> Vec<String> {
    ticked.steps.push(Step { name, began: Duration::ZERO, took: began.elapsed() });
    vec![format!("deferred {name} to the next tick: {why}")]
}
