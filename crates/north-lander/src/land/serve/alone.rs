use std::path::{Path, PathBuf};

use super::micros;
use crate::land::bisect;
use crate::land::checks::{self, Verdict};
use crate::land::lander::Lander;
use crate::land::queue::{self, Ticket};
use crate::land::records::{self, Candidate, Decision};
use crate::land::telemetry::Stages;

type Lanes = Vec<(PathBuf, Ticket)>;

pub(super) fn split(tickets: &[(PathBuf, Ticket)]) -> (Lanes, Lanes) {
    tickets.iter().cloned().partition(|(_, ticket)| ticket.red_fix.is_some() || ticket.passed_alone_on().is_some())
}

pub(super) fn check(lander: &Lander, (path, ticket): (&PathBuf, &Ticket), head: &str, base: &str, logs: &Path) -> Result<bool, String> {
    println!("north land: {} has no alone pass for its head; checking it alone on main@{base} before it joins a batch", ticket.branch);
    let log = logs.join(format!("{}-{}-alone.log", micros(), ticket.branch));
    match checks::run(lander, &lander.worktree, base, &log) {
        Verdict::Green => {
            let checked = Ticket { alone_green: Some(ticket.head.clone()), alone_base: Some(base.to_owned()), ..ticket.clone() };
            queue::write(path, &checked).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            println!("north land: {} passed alone on main@{base}; it joins the next batch", ticket.branch);
        }
        Verdict::Refused(_) => {
            let verdict = format!("failed {}", log.display());
            records::write(lander, path, ticket, &Candidate { head, batch_size: 1, stages: &Stages::empty() }, &Decision::Refused { verdict: &verdict });
            bisect::decide(path, ticket, verdict, Some(&log), base)?;
        }
        other if ticket.alone_unsettled.as_deref() == Some(ticket.head.as_str()) => {
            let (class, detail) = other.class();
            let verdict = format!("failed {}: its alone check reached no verdict twice ({class} {detail}), so it steps aside instead of holding the queue", log.display());
            records::write(lander, path, ticket, &Candidate { head, batch_size: 1, stages: &Stages::empty() }, &Decision::Refused { verdict: &verdict });
            bisect::decide(path, ticket, verdict, Some(&log), base)?;
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
