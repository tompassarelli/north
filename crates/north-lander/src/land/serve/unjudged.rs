use std::path::PathBuf;

use super::micros;
use crate::land::bisect;
use crate::land::enqueue::default_branch;
use crate::land::lander::Lander;
use crate::land::queue::{self, Ticket};
use crate::land::records::{self, Candidate, Decision};
use crate::land::scans::outgoing::denylist::{Denylist, Stand};
use crate::land::telemetry::Stages;

type Lanes = Vec<(PathBuf, Ticket)>;

const PATIENCE_MICROS: u128 = 120_000_000;

pub(super) fn settle(lander: &Lander, tickets: Lanes) -> Result<Lanes, String> {
    let mut settled = Lanes::new();
    for (path, ticket) in tickets {
        let Some(base) = ticket.unjudged_on.clone() else {
            settled.push((path, ticket));
            continue;
        };
        let verdict = Denylist::from_env().verdict_at_once(&ticket.root, &ticket.origin, &base, &ticket.head, &default_branch(&ticket.root));
        let judged = match verdict.stand {
            Stand::Pass => {
                println!("north land: the private-kind judge found nothing in {}; it joins the next batch", ticket.branch);
                Ticket { alone_green: Some(ticket.head.clone()), alone_base: Some(base), unjudged_on: None, ..ticket }
            }
            Stand::Refuse => {
                let logs = lander.queue.join("logs");
                let log = logs.join(format!("{}-{}-private-kinds.log", micros(), ticket.branch));
                std::fs::create_dir_all(&logs).and_then(|()| std::fs::write(&log, verdict.lines.join("\n") + "\n")).map_err(|error| format!("cannot write {}: {error}", log.display()))?;
                let refusal = format!("failed {}", log.display());
                records::write(lander, &path, &ticket, &Candidate { head: &ticket.head, batch_size: 1, stages: &Stages::empty() }, &Decision::Refused { verdict: &refusal });
                bisect::decide(&path, &ticket, refusal, Some(&log), &base)?;
                continue;
            }
            Stand::Judging if ticket.queued_micros.is_some_and(|queued| micros().saturating_sub(queued) < PATIENCE_MICROS) => continue,
            Stand::Judging | Stand::NoVerdict => {
                println!("north land: the private-kind judge has no verdict on {}; its alone check judges it again", ticket.branch);
                Ticket { unjudged_on: None, ..ticket }
            }
        };
        queue::write(&path, &judged).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
        settled.push((path, judged));
    }
    Ok(settled)
}
