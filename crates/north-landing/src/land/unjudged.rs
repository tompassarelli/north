use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::land::bisect;
use crate::land::lander::Lander;
use crate::land::queue::{self, HandIn, Ticket};
use crate::land::records::{self, Candidate, Decision};
use crate::land::scans::outgoing::denylist::{DenylistVerdict, Stand};
use crate::land::telemetry::Stages;

pub type Lanes = Vec<(PathBuf, Ticket)>;

pub type Verdict<'a> = dyn Fn(&Ticket, &str, Option<Duration>) -> DenylistVerdict + Sync + 'a;

const PATIENCE: Duration = Duration::from_secs(120);
const ROUND_WAIT: Duration = Duration::from_secs(10);

enum Judged {
    Ready((PathBuf, Ticket)),
    Judging((PathBuf, Ticket)),
    Decided,
}

pub fn settle(lander: &Lander, tickets: Lanes, now: Duration, verdict: &Verdict) -> Result<Lanes, String> {
    let mut settled = Lanes::new();
    let mut judging = Lanes::new();
    for (path, ticket) in tickets {
        let Some(base) = ticket.unjudged_on.clone() else {
            settled.push((path, ticket));
            continue;
        };
        let found = verdict(&ticket, &base, None);
        sort(judged(lander, path, ticket, &base, found, now)?, &mut settled, &mut judging);
    }
    if !settled.is_empty() || judging.is_empty() {
        return Ok(settled);
    }
    let started = Instant::now();
    let waited: Vec<DenylistVerdict> = std::thread::scope(|scope| {
        let asked: Vec<_> = judging.iter().map(|(_, ticket)| scope.spawn(move || awaited(ticket, now, verdict))).collect();
        asked.into_iter().map(|asked| asked.join().unwrap_or_else(|_| DenylistVerdict { lines: Vec::new(), stand: Stand::Judging })).collect()
    });
    for ((path, ticket), found) in judging.into_iter().zip(waited) {
        let base = ticket.unjudged_on.clone().unwrap_or_default();
        sort(judged(lander, path, ticket, &base, found, now + started.elapsed())?, &mut settled, &mut Lanes::new());
    }
    Ok(settled)
}

fn awaited(ticket: &Ticket, now: Duration, verdict: &Verdict) -> DenylistVerdict {
    let base = ticket.unjudged_on.as_deref().unwrap_or_default();
    verdict(ticket, base, Some(PATIENCE.saturating_sub(age(ticket, now)).min(ROUND_WAIT)))
}

fn sort(judged: Judged, settled: &mut Lanes, judging: &mut Lanes) {
    match judged {
        Judged::Ready(lane) => settled.push(lane),
        Judged::Judging(lane) => judging.push(lane),
        Judged::Decided => {}
    }
}

fn age(ticket: &Ticket, now: Duration) -> Duration {
    let now = now.as_micros();
    ticket.queued_micros.map_or(PATIENCE, |queued| Duration::from_micros(u64::try_from(now.saturating_sub(queued)).unwrap_or(u64::MAX)))
}

fn judged(lander: &Lander, path: PathBuf, ticket: Ticket, base: &str, found: DenylistVerdict, now: Duration) -> Result<Judged, String> {
    let judged = match found.stand {
        Stand::Pass => {
            println!("north land: the private-kind judge found nothing in {}; it joins the next batch", ticket.branch);
            let hand_in = ticket.hand_in.map(|hand_in| HandIn { verdict_micros: hand_in.verdict_micros.or(Some(now.as_micros())), ..hand_in });
            Ticket { alone_green: Some(ticket.head.clone()), alone_base: Some(base.to_owned()), unjudged_on: None, hand_in, ..ticket }
        }
        Stand::Refuse => {
            let logs = lander.queue.join("logs");
            let stamp = now.as_micros();
            let log = logs.join(format!("{stamp}-{}-private-kinds.log", ticket.branch));
            std::fs::create_dir_all(&logs).and_then(|()| std::fs::write(&log, found.lines.join("\n") + "\n")).map_err(|error| format!("cannot write {}: {error}", log.display()))?;
            let refusal = format!("failed {}", log.display());
            records::write(lander, &path, &ticket, &Candidate { head: &ticket.head, batch_size: 1, stages: &Stages::empty() }, &Decision::Refused { verdict: &refusal });
            bisect::decide(&path, &ticket, refusal, Some(&log), base)?;
            return Ok(Judged::Decided);
        }
        Stand::Judging if age(&ticket, now) < PATIENCE => {
            found.lines.iter().for_each(|line| println!("{line}"));
            return Ok(Judged::Judging((path, ticket)));
        }
        Stand::Judging | Stand::NoVerdict => {
            found.lines.iter().for_each(|line| println!("{line}"));
            println!("north land: the private-kind judge has no verdict on {}; its alone check judges it again", ticket.branch);
            Ticket { unjudged_on: None, ..ticket }
        }
    };
    queue::write(&path, &judged).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(Judged::Ready((path, judged)))
}

#[cfg(test)]
#[path = "unjudged_tests.rs"]
mod tests;
