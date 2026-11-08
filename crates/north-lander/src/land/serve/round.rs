use std::ops::ControlFlow;
use std::path::Path;

use super::batch::{Attempt, Entry, Flow, Pass};
use super::{Pushed, Round, Serve};
use crate::land::after;
use crate::land::bisect::{self, Judgement};
use crate::land::build_lag;
use crate::land::candidate::git;
use crate::land::checks::Verdict;
use crate::land::heartbeat::Step;
use crate::land::lander::Lander;
use crate::land::push_refusal::Refusal;
use crate::land::records::{self, Candidate, Decision};
use crate::land::steps;
use crate::land::telemetry::Stages;
use crate::land::watch::{LandedLane, Watch};

pub(super) struct Landing<'a> {
    pub(super) head: &'a str,
    pub(super) tips: &'a [String],
    pub(super) worktree: &'a Path,
}

pub(super) trait Pushing {
    fn land_batch(&self, watch: &mut Watch, pass: &mut Pass, attempt: &Attempt, lanes: &[Entry], landing: &Landing) -> Result<Flow, String>;
    fn land(&self, watch: &mut Watch, pass: &mut Pass, attempt: &Attempt, landing: &Landing, included: &[Entry], stages: Stages) -> Result<ControlFlow<Flow>, String>;
}

impl Pushing for Lander {
    fn land_batch(&self, watch: &mut Watch, pass: &mut Pass, attempt: &Attempt, lanes: &[Entry], landing: &Landing) -> Result<Flow, String> {
        let id = steps::batch(&pass.round(attempt, std::collections::BTreeMap::new(), 0), lanes, None);
        println!("north land: candidate {} carries {} lanes that passed their checks before queueing, onto main@{}; pushing it; {}", landing.head, lanes.len(), attempt.base, build_lag::report());
        let mut stages = Stages::start();
        stages.attempts = pass.rebuilds + 1;
        stages.checked(landing.head);
        steps::judge(&steps::Judged { verdict: &Verdict::Green, included: lanes, base_moved: false, streak: 0, searched: false }, &Judgement::Land, &id, 0);
        if let ControlFlow::Break(flow) = self.land(watch, pass, attempt, landing, lanes, stages)? {
            return Ok(flow);
        }
        Ok(Flow::Done(Round::Progress))
    }

    fn land(&self, watch: &mut Watch, pass: &mut Pass, attempt: &Attempt, landing: &Landing, included: &[Entry], mut stages: Stages) -> Result<ControlFlow<Flow>, String> {
        let (base, head) = (attempt.base, landing.head);
        if git(landing.worktree, &["rev-parse", "HEAD"])? != head {
            return Ok(ControlFlow::Break(Flow::Done(Round::Waiting(format!("the lander worktree moved while candidate {head} was built; nothing pushed")))));
        }
        if included.iter().any(|(path, _)| !path.exists()) {
            println!("north land: a lane of candidate {head} was decided meanwhile; rebuilding");
            return Ok(ControlFlow::Break(pass.rebuilt()));
        }
        self.heartbeat.enter(Step::Push);
        let token = match self.push_token(base) {
            Ok(token) => token,
            Err(error) => return Ok(ControlFlow::Break(Flow::Done(Round::Waiting(format!("transient: no push credential: {error}"))))),
        };
        after::begin(self, head, base, included, landing.tips)?;
        stages.pushing();
        match self.push(head, base, &token)? {
            Pushed::Landed => {}
            Pushed::Moved => {
                after::abandon(self);
                println!("north land: main moved from {base} before the push; rebuilding the candidate");
                return Ok(ControlFlow::Break(pass.rebuilt()));
            }
            Pushed::Refused(Refusal::Terminal(line)) => {
                after::abandon(self);
                let refusal = format!("failed origin refused the push of {head}: {line}");
                for (path, ticket) in included {
                    records::write(self, path, ticket, &Candidate { head, batch_size: included.len(), stages: &stages }, &Decision::Refused { verdict: &refusal });
                    bisect::decide(path, ticket, refusal.clone(), None, base)?;
                }
                return Ok(ControlFlow::Break(Flow::Done(Round::Progress)));
            }
            Pushed::Refused(Refusal::Retry(line)) => {
                after::abandon(self);
                return Ok(ControlFlow::Break(Flow::Done(Round::PushRefused(format!("origin refused the push of {head} although main is still {base}: {line}")))));
            }
        }
        stages.mark("push");
        watch.follow(self, base, head, included.iter().zip(landing.tips).map(|((_, ticket), tip)| LandedLane { ticket: (*ticket).clone(), tip: tip.clone() }).collect());
        self.heartbeat.enter(Step::Finish);
        after::finish(self, &after::intent_path(self), head, base, &stages, included.iter().enumerate().map(|(at, (path, ticket))| ((*path).clone(), (*ticket).clone(), landing.tips.get(at).map_or_else(|| head.to_owned(), Clone::clone))).collect())?;
        Ok(ControlFlow::Continue(()))
    }
}

