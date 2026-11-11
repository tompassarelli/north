use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::land::bisect;
use crate::land::lander::Lander;
use crate::land::queue::{self, HandIn, Ticket};
use crate::land::records::{self, Candidate, Decision};
use crate::land::scans::outgoing::denylist::{DenylistVerdict, Stand};
use crate::land::telemetry::Stages;

pub type Lanes = Vec<(PathBuf, Ticket)>;

pub type Verdict = dyn Fn(&Ticket, &str, Option<Duration>) -> DenylistVerdict + Send + Sync;

const PATIENCE: Duration = Duration::from_secs(120);
const REASK: Duration = Duration::from_secs(2);

struct Ask {
    head: String,
    answer: JoinHandle<DenylistVerdict>,
}

#[derive(Default)]
pub struct Asks {
    asking: HashMap<PathBuf, Ask>,
    resting: HashMap<PathBuf, Instant>,
}

impl Asks {
    pub fn running(&self) -> usize {
        self.asking.values().filter(|ask| !ask.answer.is_finished()).count()
    }

    pub fn settle(&mut self, lander: &Lander, tickets: Lanes, now: Duration, verdict: &Arc<Verdict>) -> Result<Lanes, String> {
        self.asking.retain(|path, _| tickets.iter().any(|(queued, ticket)| queued == path && ticket.unjudged_on.is_some()));
        self.resting.retain(|path, _| tickets.iter().any(|(queued, _)| queued == path));
        let mut settled = Lanes::new();
        for (path, ticket) in tickets {
            let Some(base) = ticket.unjudged_on.clone() else {
                settled.push((path, ticket));
                continue;
            };
            let Some(found) = self.answer(&path, &ticket, now, verdict) else { continue };
            if let Some(lane) = judged(lander, path, ticket, &base, found, now)? {
                settled.push(lane);
            }
        }
        Ok(settled)
    }

    fn answer(&mut self, path: &Path, ticket: &Ticket, now: Duration, verdict: &Arc<Verdict>) -> Option<DenylistVerdict> {
        let found = match self.asking.remove(path) {
            Some(ask) if ask.head == ticket.head && !ask.answer.is_finished() => {
                self.asking.insert(path.to_owned(), ask);
                return None;
            }
            Some(ask) if ask.head == ticket.head => ask.answer.join().unwrap_or_else(|_| DenylistVerdict { lines: Vec::new(), stand: Stand::Judging }),
            _ => {
                if self.resting.get(path).is_none_or(|until| Instant::now() >= *until) {
                    self.resting.remove(path);
                    self.ask(path, ticket, now, verdict);
                }
                return None;
            }
        };
        if found.stand == Stand::Judging {
            self.resting.insert(path.to_owned(), Instant::now() + REASK);
        }
        Some(found)
    }

    fn ask(&mut self, path: &Path, ticket: &Ticket, now: Duration, verdict: &Arc<Verdict>) {
        let (asked, verdict, wait) = (ticket.clone(), Arc::clone(verdict), PATIENCE.saturating_sub(age(ticket, now)));
        let answer = std::thread::spawn(move || verdict(&asked, asked.unjudged_on.as_deref().unwrap_or_default(), Some(wait)));
        self.asking.insert(path.to_owned(), Ask { head: ticket.head.clone(), answer });
    }
}

fn age(ticket: &Ticket, now: Duration) -> Duration {
    let now = now.as_micros();
    ticket.queued_micros.map_or(PATIENCE, |queued| Duration::from_micros(u64::try_from(now.saturating_sub(queued)).unwrap_or(u64::MAX)))
}

fn judged(lander: &Lander, path: PathBuf, ticket: Ticket, base: &str, found: DenylistVerdict, now: Duration) -> Result<Option<(PathBuf, Ticket)>, String> {
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
            return Ok(None);
        }
        Stand::Judging if age(&ticket, now) < PATIENCE => {
            found.lines.iter().for_each(|line| println!("{line}"));
            return Ok(None);
        }
        Stand::Judging | Stand::NoVerdict => {
            found.lines.iter().for_each(|line| println!("{line}"));
            println!("north land: the private-kind judge has no verdict on {}; its alone check judges it again", ticket.branch);
            Ticket { unjudged_on: None, ..ticket }
        }
    };
    queue::write(&path, &judged).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(Some((path, judged)))
}

#[cfg(test)]
#[path = "unjudged_tests.rs"]
mod tests;
