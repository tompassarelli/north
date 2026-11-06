use std::collections::BTreeMap;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};

use super::{ATTEMPTS, Pushed, Round, micros};
use crate::land::{after, alone};
use crate::land::bisect::{self, Judgement, Strikes};
use crate::land::candidate::{self, Built, Pick};
use crate::land::checks::Verdict;
use crate::land::culprit::{self, Found};
use crate::land::heartbeat::Step;
use crate::land::lander::Lander;
use crate::land::push_refusal::Refusal;
use crate::land::queue::{self, Ticket};
use crate::land::records::{self, Candidate, Decision};
use crate::land::telemetry::Stages;
use crate::land::watch::{LandedLane, Watch};
use crate::land::{incidents, jobs, pause, red, speculate, steps};
use crate::platform::decision_log;

type Entry<'a> = (&'a PathBuf, &'a Ticket);

enum Flow {
    Next,
    Stop,
    Done(Round),
}

struct Pass {
    limit: usize,
    red_only: bool,
    rebuilds: usize,
    bisect_depth: usize,
    searched: Option<String>,
    paused: Option<String>,
    progressed: bool,
}

impl Pass {
    fn new() -> Self {
        Self { limit: usize::MAX, red_only: false, rebuilds: 0, bisect_depth: 0, searched: None, paused: None, progressed: false }
    }

    fn progress_or(&self, otherwise: Round) -> Round {
        if self.progressed { Round::Progress } else { otherwise }
    }

    fn rebuilt(&mut self) -> Flow {
        self.rebuilds += 1;
        if self.rebuilds >= ATTEMPTS { Flow::Stop } else { Flow::Next }
    }

    fn round<'a>(&'a self, attempt: &Attempt<'a>, strikes: BTreeMap<String, north_strategy_types::land::Streak>, spares: usize) -> steps::Round<'a> {
        steps::Round { base: attempt.base, tickets: attempt.tickets, built: attempt.built, limit: self.limit, red_only: self.red_only, pause: self.paused.as_deref(), strikes, spares, rebuilds: self.rebuilds as u64, bisect_depth: self.bisect_depth as u64 }
    }
}

struct Attempt<'a> {
    base: &'a str,
    tickets: &'a [(PathBuf, Ticket)],
    built: &'a Built,
    logs: &'a Path,
}

struct Batch<'a> {
    lanes: Vec<Entry<'a>>,
    head: String,
    strikes: Strikes,
    ids: Vec<String>,
    id: String,
}

struct Checked {
    stacked: Vec<speculate::Next>,
    stages: Stages,
    log: PathBuf,
}

impl Lander {
    pub(super) fn batch(&self, watch: &mut Watch) -> Result<Round, String> {
        let logs = self.queue.join("logs");
        let mut pass = Pass::new();
        self.round.store(u64::try_from(micros()).unwrap_or(u64::MAX), std::sync::atomic::Ordering::Relaxed);
        self.heartbeat.enter(Step::Jobs);
        watch.jobs.start(self);
        let answering = watch.jobs.answering(self);
        loop {
            match self.attempt(watch, &mut pass, &logs, answering)? {
                Flow::Next => {}
                Flow::Stop => break,
                Flow::Done(round) => return Ok(round),
            }
        }
        Ok(pass.progress_or(Round::Waiting(format!("no candidate settled in {ATTEMPTS} attempts"))))
    }

    fn attempt(&self, watch: &mut Watch, pass: &mut Pass, logs: &Path, answering: bool) -> Result<Flow, String> {
        let claimed = jobs::claimed(self);
        let mut queued = queue::collect(&self.queue).map_err(|error| error.to_string())?;
        queued.retain(|(path, _)| !claimed.contains(path));
        let fixes;
        let tickets = if pass.red_only {
            fixes = red::fixes(&queued);
            &fixes
        } else {
            &queued
        };
        if let Some(flow) = self.gate(pass, &queued, tickets.is_empty(), answering) {
            return Ok(flow);
        }
        self.heartbeat.enter(Step::Prepare);
        let base = match candidate::prepare(&self.repo, &self.worktree) {
            Ok(base) => base,
            Err(error) => return Ok(Flow::Done(Round::Waiting(format!("transient: {error}")))),
        };
        self.heartbeat.enter(Step::Build);
        let built = candidate::build(&self.worktree, &base, tickets, &queued, pass.limit, logs)?;
        let attempt = Attempt { base: &base, tickets, built: &built, logs };
        let (included, undecided) = self.sort_picks(pass, &attempt)?;
        if included.is_empty() {
            steps::batch(&pass.round(&attempt, BTreeMap::new(), self.spares.len()), &included, None);
            return Ok(Flow::Done(match (pass.progressed, undecided) {
                (true, _) => Round::Progress,
                (false, Some(reason)) => Round::Waiting(reason),
                (false, None) => Round::Idle,
            }));
        }
        self.check_included(watch, pass, &attempt, included)
    }

    fn gate(&self, pass: &mut Pass, queued: &[(PathBuf, Ticket)], empty: bool, answering: bool) -> Option<Flow> {
        if empty {
            return Some(Flow::Done(pass.progress_or(Round::Idle)));
        }
        if answering && !pass.red_only {
            if red::fixes(queued).is_empty() {
                return Some(Flow::Done(pass.progress_or(Round::Checking)));
            }
            println!("north land: main is red and the sheriff is answering its revert request; checking the queued red fixes alone");
            pass.red_only = true;
            return Some(Flow::Next);
        }
        if !pass.red_only && let Some(reason) = pause::held(&self.queue) {
            if red::fixes(queued).is_empty() {
                return Some(Flow::Done(Round::Waiting(reason)));
            }
            println!("north land: {reason}; checking the queued red fixes alone");
            pass.paused = Some(reason);
            pass.red_only = true;
            return Some(Flow::Next);
        }
        None
    }

    fn sort_picks<'a>(&self, pass: &mut Pass, attempt: &Attempt<'a>) -> Result<(Vec<Entry<'a>>, Option<String>), String> {
        let mut included = Vec::new();
        let mut undecided = None;
        for ((path, ticket), pick) in attempt.tickets.iter().zip(&attempt.built.picks) {
            match pick {
                Pick::Included => included.push((path, ticket)),
                Pick::Decided(verdict) => {
                    if let Some(sha) = verdict.strip_prefix("landed ") {
                        after::cleanup::report(self, ticket, sha);
                    }
                    records::write(self, path, ticket, &Candidate { head: "", batch_size: 0, stages: &Stages::empty() }, &Decision::Refused { verdict });
                    bisect::decide(path, ticket, verdict.clone(), None, attempt.base)?;
                    pass.progressed = true;
                }
                Pick::Refused(refused) => {
                    records::write(self, path, ticket, &Candidate { head: "", batch_size: 0, stages: &Stages::empty() }, &Decision::Refused { verdict: &refused.verdict });
                    bisect::decide(path, ticket, refused.verdict.clone(), Some(&refused.log), attempt.base)?;
                    pass.progressed = true;
                }
                Pick::Undecided(reason) => undecided = Some(reason.clone()),
                Pick::Deferred(reason) => println!("north land: {reason}"),
            }
        }
        Ok((included, undecided))
    }

    fn check_included<'a>(&self, watch: &mut Watch, pass: &mut Pass, attempt: &Attempt<'a>, lanes: Vec<Entry<'a>>) -> Result<Flow, String> {
        let strikes = Strikes::load(&self.queue, attempt.base)?;
        let ids = Strikes::lanes(&lanes);
        let asked = alone::take(&self.queue) && lanes.len() > 1;
        let halve = (lanes.len() > 1 && (asked || strikes.exhausted(&ids))).then(|| bisect::half(lanes.len()));
        let id = steps::batch(&pass.round(attempt, strikes.streaks(), self.spares.len()), &lanes, halve);
        if let Some(half) = halve {
            println!("north land: {} of this candidate's {} lanes kept ending without a verdict; checking each lane alone", if asked { "the watchdog asked for it:" } else { "a lane" }, lanes.len());
            let log = attempt.logs.join(format!("{}-candidate.log", micros()));
            let mut checked = Checked { stacked: Vec::new(), stages: Stages::start(), log };
            let mut batch = Batch { lanes, head: attempt.built.head.clone(), strikes, ids, id };
            let transient = Verdict::Transient("lanes that kept ending without a verdict".to_owned());
            let Some(judgement) = self.search_culprit(pass, attempt, &mut batch, Judgement::Halve(half), transient, &mut checked)? else {
                return Ok(Flow::Next);
            };
            return self.settle(watch, pass, attempt, batch, checked, judgement);
        }
        let (verdict, mut checked) = match self.check(pass, attempt, &lanes)? {
            ControlFlow::Continue(checked) => checked,
            ControlFlow::Break(round) => return Ok(Flow::Done(round)),
        };
        let mut batch = Batch { lanes, head: attempt.built.head.clone(), strikes, ids, id };
        let (judged, refusal) = match self.judge(pass, attempt, &mut batch, verdict, &checked)? {
            ControlFlow::Continue(judged) => judged,
            ControlFlow::Break(round) => return Ok(Flow::Done(round)),
        };
        let Some(judgement) = self.search_culprit(pass, attempt, &mut batch, judged, refusal, &mut checked)? else {
            return Ok(Flow::Next);
        };
        self.settle(watch, pass, attempt, batch, checked, judgement)
    }

    fn check(&self, pass: &Pass, attempt: &Attempt, included: &[Entry]) -> Result<ControlFlow<Round, (Verdict, Checked)>, String> {
        let built = attempt.built;
        let identity = self.identity()?;
        println!("north land: candidate {} carries {} lanes onto main@{}; checking it", built.head, built.included(), attempt.base);
        let log = attempt.logs.join(format!("{}-candidate.log", micros()));
        let mut stages = Stages::start();
        let taken = (pass.limit == usize::MAX && !pass.red_only && !self.spares.is_empty()).then(|| speculate::Taken::new(attempt.tickets, included));
        self.heartbeat.enter(Step::Check);
        let (verdict, stacked) = speculate::check(self, &built.head, attempt.base, &log, taken.as_ref(), included.iter().any(|(_, ticket)| ticket.red_fix.is_some()));
        stages.checked(&built.head);
        if matches!(verdict, Verdict::Green | Verdict::Refused(_)) {
            pause::resume(&self.queue);
        }
        stages.attempts = pass.rebuilds + 1;
        stages.bisect_depth = pass.bisect_depth;
        if (self.identity()? != identity || identity.0 != built.head) && !matches!(verdict, Verdict::Refused(_)) {
            return Ok(ControlFlow::Break(Round::Waiting(format!("the lander worktree moved while candidate {} was checked; nothing pushed", built.head))));
        }
        Ok(ControlFlow::Continue((verdict, Checked { stacked, stages, log })))
    }

    fn judge(&self, pass: &Pass, attempt: &Attempt, batch: &mut Batch, verdict: Verdict, checked: &Checked) -> Result<ControlFlow<Round, (Judgement, Verdict)>, String> {
        let moved = match &verdict {
            Verdict::Green => false,
            _ => match self.base_moved(attempt.base) {
                Ok(moved) => moved,
                Err(error) => return Ok(ControlFlow::Break(Round::Waiting(format!("transient: {error}")))),
            },
        };
        let streak = if moved { 0 } else { batch.strikes.record(&batch.ids, &verdict) };
        let verdict = bisect::escalate(verdict, streak, &checked.log);
        batch.strikes.save(&self.queue)?;
        let (judged, decide_us) = decision_log::timed(|| bisect::judge(verdict.clone(), batch.lanes.len(), moved));
        steps::judge(&steps::Judged { verdict: &verdict, included: &batch.lanes, base_moved: moved, streak, searched: pass.searched.as_deref() == Some(attempt.base) }, &judged, &batch.id, decide_us);
        Ok(ControlFlow::Continue((judged, verdict)))
    }

    fn search_culprit(&self, pass: &mut Pass, attempt: &Attempt, batch: &mut Batch, judged: Judgement, refusal: Verdict, checked: &mut Checked) -> Result<Option<Judgement>, String> {
        let Judgement::Halve(_) = judged else { return Ok(Some(judged)) };
        if pass.searched.as_deref() == Some(attempt.base) {
            return Ok(Some(judged));
        }
        checked.stacked.clear();
        pass.searched = Some(attempt.base.to_owned());
        let together = culprit::Checked { verdict: refusal, head: attempt.built.head.clone(), log: checked.log.clone() };
        self.heartbeat.enter(Step::Culprit);
        Ok(match culprit::search(self, attempt.base, &batch.lanes, together, attempt.logs, &mut batch.strikes, &checked.stages)? {
            Found::Decided => {
                pass.progressed = true;
                pass.limit = usize::MAX;
                pass.bisect_depth += 1;
                None
            }
            Found::Interaction { lanes, head: prefix } => {
                batch.lanes.truncate(lanes);
                batch.head = prefix;
                Some(Judgement::Land)
            }
            Found::Moved => Some(Judgement::Rebuild),
            Found::Held(reason) => Some(Judgement::Hold(reason)),
            Found::Paused(detail) => Some(Judgement::Paused(detail)),
        })
    }

    fn settle(&self, watch: &mut Watch, pass: &mut Pass, attempt: &Attempt, batch: Batch, checked: Checked, judgement: Judgement) -> Result<Flow, String> {
        let (base, built) = (attempt.base, attempt.built);
        match judgement {
            Judgement::Land => self.land(watch, pass, attempt, batch, checked),
            Judgement::Rebuild => {
                println!("north land: main moved from {base} while candidate {} was checked; rebuilding it, no lane judged", built.head);
                pass.limit = usize::MAX;
                Ok(pass.rebuilt())
            }
            Judgement::Halve(half) => {
                println!("north land: candidate {} refused; checking its first {half} lanes", built.head);
                pass.limit = half;
                pass.bisect_depth += 1;
                Ok(Flow::Next)
            }
            Judgement::Culprit(detail) => {
                let (path, ticket) = batch.lanes[0];
                let refusal = format!("failed {detail}");
                records::write(self, path, ticket, &Candidate { head: &built.head, batch_size: batch.lanes.len(), stages: &checked.stages }, &Decision::Refused { verdict: &refusal });
                bisect::decide(path, ticket, refusal, Some(&checked.log), base)?;
                Ok(Flow::Done(Round::Progress))
            }
            Judgement::RedMain(detail) if !pass.red_only && red::narrows(&batch.lanes) => {
                println!("north land: main is red ({detail}); checking the red-fix lanes alone first");
                pass.red_only = true;
                pass.limit = usize::MAX;
                Ok(Flow::Next)
            }
            Judgement::RedMain(detail) => Ok(Flow::Done(Round::Waiting(red::held(&detail, batch.lanes.len())))),
            Judgement::Hold(reason) => {
                let (path, ticket) = batch.lanes[0];
                incidents::record(&self.queue, path, ticket, incidents::held(&built.head, base, &reason));
                incidents::attribute(ticket, &reason);
                Ok(Flow::Done(Round::Waiting(reason)))
            }
            Judgement::Paused(detail) => Ok(Flow::Done(Round::Waiting(pause::raise(&self.queue, &detail)?))),
        }
    }

    fn land(&self, watch: &mut Watch, pass: &mut Pass, attempt: &Attempt, batch: Batch, checked: Checked) -> Result<Flow, String> {
        let Checked { stacked, mut stages, log } = checked;
        let (included, head) = (batch.lanes, batch.head.as_str());
        let (base, built) = (attempt.base, attempt.built);
        self.heartbeat.enter(Step::Push);
        let token = match self.push_token(base) {
            Ok(token) => token,
            Err(error) => return Ok(Flow::Done(Round::Waiting(format!("transient: no push credential: {error}")))),
        };
        after::begin(self, head, base, &included, &built.tips)?;
        stages.pushing();
        match self.push(head, base, token.as_deref())? {
            Pushed::Landed => {}
            Pushed::Moved => {
                after::abandon(self);
                println!("north land: main moved from {base} before the push; rebuilding the candidate");
                return Ok(pass.rebuilt());
            }
            Pushed::Refused(Refusal::Terminal(line)) => {
                after::abandon(self);
                let refusal = format!("failed origin refused the push of {head}: {line}");
                let batch_size = included.len();
                for (path, ticket) in included {
                    records::write(self, path, ticket, &Candidate { head, batch_size, stages: &stages }, &Decision::Refused { verdict: &refusal });
                    bisect::decide(path, ticket, refusal.clone(), None, base)?;
                }
                return Ok(Flow::Done(Round::Progress));
            }
            Pushed::Refused(Refusal::Retry(line)) => {
                after::abandon(self);
                return Ok(Flow::Done(Round::PushRefused(format!("origin refused the push of {head} although main is still {base}: {line}"))));
            }
        }
        stages.mark("push");
        records::coverage(&self.queue, &built.tips[..included.len().min(built.tips.len())], &log);
        watch.follow(self, base, head, included.iter().zip(&built.tips).map(|((_, ticket), tip)| LandedLane { ticket: (*ticket).clone(), tip: tip.clone() }).collect());
        self.heartbeat.enter(Step::Finish);
        after::finish(self, &after::intent_path(self), head, base, &stages, included.into_iter().map(|(path, ticket)| (path.clone(), ticket.clone())).collect())?;
        self.heartbeat.enter(Step::Speculate);
        speculate::land(self, watch, stacked, head)?;
        Ok(Flow::Done(Round::Progress))
    }
}
