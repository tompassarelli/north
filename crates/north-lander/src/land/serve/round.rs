use std::ops::ControlFlow;
use std::path::Path;

use north_check_environment::{self as check_environment, Reading};
use north_land_probes::{Settled, plan, settle};
use serde::{Deserialize, Serialize};

use super::batch::{Attempt, Entry, Flow, Pass};
use super::{Pushed, Round, Serve};
use crate::land::after;
use crate::land::bisect::{self, Judgement, Strikes};
use crate::land::candidate::git;
use crate::land::checks::Verdict;
use crate::land::heartbeat::Step;
use crate::land::lander::Lander;
use crate::land::probe_slots;
use crate::land::probes::{self, End, Probe};
use crate::land::push_refusal::Refusal;
use crate::land::queue;
use crate::land::records::{self, Candidate, Decision};
use crate::land::telemetry::Stages;
use crate::land::watch::{LandedLane, Watch};
use crate::land::{incidents, pause, red, steps};

const SUSPECTS: &str = "suspects";
const STALLS_BEFORE_ALONE: u32 = 2;

#[derive(Default, Serialize, Deserialize)]
struct Suspects {
    ids: Vec<String>,
    stalls: u32,
}

impl Suspects {
    fn load(queue: &Path) -> Self {
        std::fs::read_to_string(queue.join(SUSPECTS)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    fn save(&self, queue: &Path) -> Result<(), String> {
        let text = serde_json::to_string(self).map_err(|error| error.to_string())?;
        queue::write_atomic(&queue.join(SUSPECTS), &text).map_err(|error| error.to_string())
    }

    fn plan(&self, ids: &[String], room: usize) -> Vec<Vec<usize>> {
        let named: Vec<usize> = ids.iter().enumerate().filter(|(_, id)| self.ids.contains(id)).map(|(at, _)| at).collect();
        if self.stalls >= STALLS_BEFORE_ALONE && let Some(first) = named.first() {
            return vec![vec![*first]];
        }
        let room = if ids.len() > 1 { room.max(2) } else { room };
        plan(&(0..ids.len()).collect::<Vec<_>>(), room, usize::MAX, &named)
    }

    fn after(&mut self, ids: &[String], next: &[usize], landed: bool) {
        let next: Vec<String> = next.iter().map(|lane| ids[*lane].clone()).collect();
        self.stalls = if landed || next != self.ids { 0 } else { self.stalls + 1 };
        self.ids = next;
    }
}

fn reading(finished: &[Probe]) -> Reading {
    let verdicts = || finished.iter().filter_map(Probe::verdict);
    if let Some(Verdict::BrokenBase(detail)) = verdicts().find(|verdict| matches!(verdict, Verdict::BrokenBase(_))) {
        return Reading::Broken(detail.clone());
    }
    if verdicts().any(|verdict| matches!(verdict, Verdict::Green | Verdict::Refused(_) | Verdict::RedMain(_))) { Reading::Reached } else { Reading::Silent }
}

fn escalate(finished: &mut [Probe], lanes: &[Entry], strikes: &mut Strikes) {
    let ids = Strikes::lanes(lanes);
    let before = strikes.streaks();
    let run = |lane: &String, verdict: &Verdict| before.get(lane).map_or(0, |streak| if matches!(verdict, Verdict::Transient(_)) { streak.transient } else { streak.deferred });
    for probe in finished.iter_mut() {
        let Some(verdict) = probe.verdict().cloned() else { continue };
        let streak = 1 + probe.lanes.iter().map(|lane| run(&ids[*lane], &verdict)).max().unwrap_or(0);
        probe.end = End::Checked(bisect::escalate(verdict, streak, &probe.log));
    }
    for (at, id) in ids.iter().enumerate() {
        let seen: Vec<&Verdict> = finished.iter().filter(|probe| probe.lanes.contains(&at)).filter_map(Probe::verdict).collect();
        if let Some(verdict) = seen.iter().find(|verdict| matches!(verdict, Verdict::Green | Verdict::Refused(_))).or(seen.first()) {
            strikes.record(std::slice::from_ref(id), verdict);
        }
    }
}

pub(super) struct Decided<'a> {
    attempt: &'a Attempt<'a>,
    lanes: &'a [Entry<'a>],
    probes: Vec<&'a Probe>,
    settled: Settled<usize>,
    id: String,
}

impl Decided<'_> {
    fn landing(&self) -> Option<&Probe> {
        self.probes.iter().find(|probe| probe.green() && probe.lanes == self.settled.land).copied()
    }

    fn proof(&self, culprit: usize) -> Option<&Probe> {
        let carries = |probe: &&&Probe| !probe.green() && self.settled.land.iter().all(|lane| probe.lanes.contains(lane)) && probe.lanes.contains(&culprit);
        self.probes.iter().filter(carries).min_by_key(|probe| probe.lanes.len()).copied()
    }
}

pub(super) trait Probing {
    fn probe_round(&self, watch: &mut Watch, pass: &mut Pass, attempt: &Attempt, lanes: &[Entry]) -> Result<Flow, String>;
    fn settle_decided(&self, watch: &mut Watch, pass: &mut Pass, decided: &Decided, stages: Stages) -> Result<Flow, String>;
    fn settle_undecided(&self, pass: &mut Pass, lanes: &[Entry], finished: &[Probe]) -> Result<Flow, String>;
    fn refuse_culprit(&self, pass: &Pass, lane: Entry, proof: &Probe, main: &str) -> Result<(), String>;
    fn land_probe(&self, watch: &mut Watch, pass: &mut Pass, attempt: &Attempt, probe: &Probe, lanes: &[Entry], stages: Stages) -> Result<ControlFlow<Flow>, String>;
}

impl Probing for Lander {
    fn probe_round(&self, watch: &mut Watch, pass: &mut Pass, attempt: &Attempt, lanes: &[Entry]) -> Result<Flow, String> {
        let ids = Strikes::lanes(lanes);
        let mut strikes = Strikes::load(&self.queue, &ids)?;
        let (room, read) = probe_slots::slots(&self.repo);
        let mut suspects = Suspects::load(&self.queue);
        let planned = suspects.plan(&ids, room);
        let id = steps::batch(&pass.round(attempt, strikes.streaks(), room), lanes, None);
        steps::wave(lanes.len(), read, planned.len());
        println!("north land: probing {} lanes of main@{} with candidates of {} lanes", lanes.len(), attempt.base, planned.iter().map(|probe| probe.len().to_string()).collect::<Vec<_>>().join(", "));
        let mut stages = Stages::start();
        self.heartbeat.enter(Step::Check);
        let mut finished = check_environment::judge(&self.queue, |environment| {
            let finished = probes::run(self, watch, attempt.base, attempt.logs, lanes, &planned, environment.is_local());
            let reading = reading(&finished);
            (finished, reading)
        });
        for probe in &finished {
            if let (Some(verdict), Some(&(path, ticket))) = (probe.verdict(), probe.lanes.first().map(|lane| &lanes[*lane])) {
                records::unsettled(&self.queue, path, ticket, &probe.head, attempt.base, verdict.class(), &probe.log);
            }
        }
        escalate(&mut finished, lanes, &mut strikes);
        strikes.save(&self.queue)?;
        stages.attempts = pass.rebuilds + 1;
        let probes: Vec<&Probe> = finished.iter().filter(|probe| probe.green() || probe.refused()).collect();
        if probes.is_empty() {
            return self.settle_undecided(pass, lanes, &finished);
        }
        pause::resume(&self.queue);
        let settled = settle(&probes.iter().map(|probe| probe.lanes.clone()).collect::<Vec<_>>(), &probes.iter().map(|probe| probe.green()).collect::<Vec<_>>());
        suspects.after(&ids, &settled.suspects, !settled.land.is_empty() || settled.refuse.is_some());
        suspects.save(&self.queue)?;
        if settled.land.is_empty() && self.base_moved(attempt.base).map_err(|error| format!("transient: {error}"))? {
            println!("north land: main moved from {} while the probes were checked; rebuilding, no lane judged", attempt.base);
            return Ok(pass.rebuilt());
        }
        self.settle_decided(watch, pass, &Decided { attempt, lanes, probes, settled, id }, stages)
    }

    fn settle_decided(&self, watch: &mut Watch, pass: &mut Pass, decided: &Decided, mut stages: Stages) -> Result<Flow, String> {
        let (attempt, lanes, settled) = (decided.attempt, decided.lanes, &decided.settled);
        let mut main = attempt.base.to_owned();
        if let Some(landing) = decided.landing() {
            stages.checked(&landing.head);
            let landed: Vec<Entry> = landing.lanes.iter().map(|lane| lanes[*lane]).collect();
            steps::judge(&steps::Judged { verdict: &Verdict::Green, included: &landed, base_moved: false, streak: 0, searched: false }, &Judgement::Land, &decided.id, 0);
            if let ControlFlow::Break(flow) = self.land_probe(watch, pass, attempt, landing, lanes, stages)? {
                return Ok(flow);
            }
            main.clone_from(&landing.head);
        }
        if let Some(culprit) = settled.refuse
            && let Some(proof) = decided.proof(culprit)
        {
            let detail = proof.verdict().map(|verdict| verdict.class().1.to_owned()).unwrap_or_default();
            steps::judge(&steps::Judged { verdict: proof.verdict().unwrap_or(&Verdict::Green), included: &[lanes[culprit]], base_moved: false, streak: 0, searched: false }, &Judgement::Culprit(detail), &decided.id, 0);
            self.refuse_culprit(pass, lanes[culprit], proof, &main)?;
        } else if settled.land.is_empty() {
            let names = settled.suspects.iter().map(|lane| lanes[*lane].1.branch.as_str()).collect::<Vec<_>>().join(", ");
            println!("north land: probes carrying {names} were refused; the next round leaves each of them out in turn");
        }
        Ok(Flow::Done(Round::Progress))
    }

    fn settle_undecided(&self, pass: &mut Pass, lanes: &[Entry], finished: &[Probe]) -> Result<Flow, String> {
        let verdicts = || finished.iter().filter_map(Probe::verdict);
        if let Some(Verdict::BrokenBase(detail)) = verdicts().find(|verdict| matches!(verdict, Verdict::BrokenBase(_))) {
            return Ok(Flow::Done(Round::Waiting(pause::raise(&self.queue, detail)?)));
        }
        if let Some(Verdict::RedMain(detail)) = verdicts().find(|verdict| matches!(verdict, Verdict::RedMain(_))) {
            if !pass.red_only && red::narrows(lanes) {
                println!("north land: main is red ({detail}); checking the red-fix lanes alone first");
                pass.red_only = true;
                return Ok(Flow::Next);
            }
            return Ok(Flow::Done(Round::Waiting(red::held(detail, lanes.len()))));
        }
        let held = |verdict: &Verdict| match verdict {
            Verdict::Transient(detail) => Some(format!("transient {detail}")),
            Verdict::Deferred(detail) => Some(format!("deferred {detail}")),
            _ => None,
        };
        if let Some((probe, reason)) = finished.iter().find_map(|probe| probe.verdict().and_then(held).map(|reason| (probe, reason))) {
            let (_, ticket) = lanes[probe.lanes[0]];
            incidents::attribute(ticket, &reason);
            return Ok(Flow::Done(Round::Waiting(reason)));
        }
        let reason = finished.iter().find_map(|probe| if let End::Unbuilt(reason) = &probe.end { Some(reason.clone()) } else { None }).unwrap_or_else(|| "no probe reached a verdict".to_owned());
        Ok(Flow::Done(Round::Waiting(reason)))
    }

    fn refuse_culprit(&self, pass: &Pass, (path, ticket): Entry, proof: &Probe, main: &str) -> Result<(), String> {
        let Some(Verdict::Refused(detail)) = proof.verdict() else { return Ok(()) };
        let refusal = format!("failed {detail}");
        let mut stages = Stages::start();
        stages.checked(&proof.head);
        stages.attempts = pass.rebuilds + 1;
        println!("north land: {} is the culprit: the probe of {} lanes carrying it was refused", ticket.branch, proof.lanes.len());
        records::write(self, path, ticket, &Candidate { head: &proof.head, batch_size: proof.lanes.len(), stages: &stages }, &Decision::Refused { verdict: &refusal });
        bisect::decide(path, ticket, refusal, Some(&proof.log), main)
    }

    fn land_probe(&self, watch: &mut Watch, pass: &mut Pass, attempt: &Attempt, probe: &Probe, lanes: &[Entry], mut stages: Stages) -> Result<ControlFlow<Flow>, String> {
        let included: Vec<Entry> = probe.lanes.iter().map(|lane| lanes[*lane]).collect();
        let (base, head) = (attempt.base, probe.head.as_str());
        if git(&probe.worktree, &["rev-parse", "HEAD"])? != head {
            return Ok(ControlFlow::Break(Flow::Done(Round::Waiting(format!("the probe worktree moved while candidate {head} was checked; nothing pushed")))));
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
        after::begin(self, head, base, &included, &probe.tips)?;
        stages.pushing();
        match self.push(head, base, token.as_deref())? {
            Pushed::Landed => {}
            Pushed::Moved => {
                after::abandon(self);
                println!("north land: main moved from {base} before the push; rebuilding the candidate");
                return Ok(ControlFlow::Break(pass.rebuilt()));
            }
            Pushed::Refused(Refusal::Terminal(line)) => {
                after::abandon(self);
                let refusal = format!("failed origin refused the push of {head}: {line}");
                for (path, ticket) in &included {
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
        records::coverage(&self.queue, &probe.tips[..included.len().min(probe.tips.len())], &probe.log);
        watch.follow(self, base, head, included.iter().zip(&probe.tips).map(|((_, ticket), tip)| LandedLane { ticket: (*ticket).clone(), tip: tip.clone() }).collect());
        self.heartbeat.enter(Step::Finish);
        after::finish(self, &after::intent_path(self), head, base, &stages, included.into_iter().map(|(path, ticket)| (path.clone(), ticket.clone())).collect())?;
        Ok(ControlFlow::Continue(()))
    }
}
