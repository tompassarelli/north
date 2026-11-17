use std::path::{Path, PathBuf};

use super::micros;
use crate::land::after_landing_lifecycle::{self as lifecycle, Main, RedMain};
use crate::land::alone_flight::{self, Check};
use crate::land::bisect;
use crate::land::checks::Verdict;
use crate::land::lander::Lander;
use crate::land::queue::{self, Ticket};
use crate::land::records::{self, Decision};
use crate::land::watch::Watch;
use north_base::platform::push_ledger::repository;

type Lanes = Vec<(PathBuf, Ticket)>;

/// Splits the queue into lanes that join a batch without an alone round and lanes that need one. While main is red, an urgent
/// lane that fails no check main also fails joins the next batch at once; its tests run after it lands (north#1631).
pub(super) fn split(tickets: &[(PathBuf, Ticket)], main_red: bool) -> (Lanes, Lanes) {
    tickets.iter().cloned().partition(|(_, ticket)| ticket.red_fix.is_some() || ticket.passed_alone_on().is_some() || (main_red && ticket.urgent.is_some() && ticket.failed_on_main.is_empty()))
}

pub(super) fn main_red(tickets: &[(PathBuf, Ticket)]) -> bool {
    let Some((_, ticket)) = tickets.first() else { return false };
    let store = match lifecycle::host() {
        Ok(store) => store,
        Err(error) => {
            println!("{error}");
            return false;
        }
    };
    matches!(lifecycle::current::<RedMain>(&store, &repository(&ticket.origin)).map(|record| record.state), Some(Main::Opened | Main::Red))
}

pub(super) fn start(lander: &Lander, watch: &mut Watch, (path, ticket): (&PathBuf, &Ticket), candidate: &str, base: &str, worktree: &Path) -> Result<bool, String> {
    println!("north land: {} has no alone pass for its head; checking it alone on main@{base} beside the round before it joins a batch", ticket.branch);
    let log = lander.queue.join("logs").join(format!("{}-{}-alone.log", micros(), ticket.branch));
    match alone_flight::start(lander, Check { path, ticket, candidate, base, worktree, log: log.clone() }) {
        Ok(flight) => {
            watch.alone = Some(flight);
            Ok(false)
        }
        Err(verdict) => decide(lander, (path, ticket), base, &log, verdict),
    }
}

pub(super) fn settle(lander: &Lander, watch: &mut Watch) -> Result<bool, String> {
    let Some(verdict) = watch.alone.as_mut().and_then(|flight| flight.poll(&lander.queue)) else { return Ok(false) };
    let Some(flight) = watch.alone.take() else { return Ok(false) };
    let record = flight.record;
    match queue::read(&record.ticket) {
        Ok(ticket) if ticket.head == record.head => decide(lander, (&record.ticket, &ticket), &record.base, &record.log, verdict),
        _ => {
            println!("north land: {} was decided or moved on while its alone check ran; its verdict is dropped", record.branch);
            Ok(true)
        }
    }
}

fn decide(lander: &Lander, (path, ticket): (&PathBuf, &Ticket), base: &str, log: &Path, verdict: Verdict) -> Result<bool, String> {
    match verdict {
        Verdict::Green => {
            let checked = Ticket { alone_green: Some(ticket.head.clone()), alone_base: Some(base.to_owned()), ..ticket.clone() };
            queue::write(path, &checked).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            println!("north land: {} passed alone on main@{base}; it joins the next batch", ticket.branch);
        }
        Verdict::Refused(_) => {
            let verdict = format!("failed {}", log.display());
            records::settle(lander, ticket, &Decision::Refused { verdict: &verdict });
            bisect::decide(path, ticket, verdict, Some(log), base)?;
        }
        other if ticket.alone_unsettled.as_deref() == Some(ticket.head.as_str()) => {
            let (class, detail) = other.class();
            let verdict = format!("failed {}: its alone check reached no verdict twice ({class} {detail}), so it steps aside instead of holding the queue", log.display());
            records::settle(lander, ticket, &Decision::Refused { verdict: &verdict });
            bisect::decide(path, ticket, verdict, Some(log), base)?;
        }
        other => {
            let (class, detail) = other.class();
            let unsettled = Ticket { alone_unsettled: Some(ticket.head.clone()), ..ticket.clone() };
            queue::write(path, &unsettled).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            println!("north land: the alone check of {} reached no verdict ({class} {detail}); it is checked once more next round", ticket.branch);
            return Ok(false);
        }
    }
    Ok(true)
}
