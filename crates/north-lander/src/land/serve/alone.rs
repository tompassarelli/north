use std::path::{Path, PathBuf};

use super::micros;
use crate::land::alone_flight::{self, Check};
use crate::land::bisect;
use crate::land::checks::Verdict;
use crate::land::lander::Lander;
use crate::land::queue::{self, Ticket};
use crate::land::records::{self, Candidate, Decision};
use crate::land::telemetry::Stages;
use crate::land::watch::Watch;

type Lanes = Vec<(PathBuf, Ticket)>;

pub(super) fn split(tickets: &[(PathBuf, Ticket)]) -> (Lanes, Lanes) {
    tickets.iter().cloned().partition(|(_, ticket)| ticket.red_fix.is_some() || ticket.passed_alone_on().is_some())
}

pub(super) fn start(lander: &Lander, watch: &mut Watch, (path, ticket): (&PathBuf, &Ticket), candidate: &str, base: &str, worktree: &Path) -> Result<bool, String> {
    println!("north land: {} has no alone pass for its head; checking it alone on main@{base} beside the round before it joins a batch", ticket.branch);
    let log = lander.queue.join("logs").join(format!("{}-{}-alone.log", micros(), ticket.branch));
    match alone_flight::start(lander, Check { path, ticket, candidate, base, worktree, log: log.clone() }) {
        Ok(flight) => {
            watch.alone = Some(flight);
            Ok(false)
        }
        Err(verdict) => decide(lander, (path, ticket), candidate, base, &log, verdict),
    }
}

pub(super) fn settle(lander: &Lander, watch: &mut Watch) -> Result<bool, String> {
    let Some(verdict) = watch.alone.as_mut().and_then(|flight| flight.poll(&lander.queue)) else { return Ok(false) };
    let Some(flight) = watch.alone.take() else { return Ok(false) };
    let record = flight.record;
    match queue::read(&record.ticket) {
        Ok(ticket) if ticket.head == record.head => decide(lander, (&record.ticket, &ticket), &record.candidate, &record.base, &record.log, verdict),
        _ => {
            println!("north land: {} was decided or moved on while its alone check ran; its verdict is dropped", record.branch);
            Ok(true)
        }
    }
}

fn decide(lander: &Lander, (path, ticket): (&PathBuf, &Ticket), head: &str, base: &str, log: &Path, verdict: Verdict) -> Result<bool, String> {
    match verdict {
        Verdict::Green => {
            let checked = Ticket { alone_green: Some(ticket.head.clone()), alone_base: Some(base.to_owned()), ..ticket.clone() };
            queue::write(path, &checked).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            println!("north land: {} passed alone on main@{base}; it joins the next batch", ticket.branch);
        }
        Verdict::Refused(_) => {
            let verdict = format!("failed {}", log.display());
            records::write(lander, path, ticket, &Candidate { head, batch_size: 1, stages: &Stages::empty() }, &Decision::Refused { verdict: &verdict });
            bisect::decide(path, ticket, verdict, Some(log), base)?;
        }
        other if ticket.alone_unsettled.as_deref() == Some(ticket.head.as_str()) => {
            let (class, detail) = other.class();
            let verdict = format!("failed {}: its alone check reached no verdict twice ({class} {detail}), so it steps aside instead of holding the queue", log.display());
            records::write(lander, path, ticket, &Candidate { head, batch_size: 1, stages: &Stages::empty() }, &Decision::Refused { verdict: &verdict });
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
