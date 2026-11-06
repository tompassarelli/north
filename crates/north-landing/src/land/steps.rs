//! The lander's steps as decision-log lines: each candidate round, each judged candidate, each culprit
//! wave and each sheriff bisection step, with the observations it was taken on.
use std::collections::BTreeMap;
use std::path::PathBuf;

use north_strategy_types::Kind;
use north_strategy_types::land::{self, BatchCase, BatchDecision, JudgeCase};

use super::bisect::Judgement;
use super::candidate::{self, Built};
use super::checks::Verdict;
use super::queue::Ticket;
use crate::platform::decision_log::{self, Entry};
use crate::platform::github_quota;

/// The lander's state in one round of `batch`, beside the queue and the candidate built from it.
pub struct Round<'a> {
    pub base: &'a str,
    pub tickets: &'a [(PathBuf, Ticket)],
    pub built: &'a Built,
    pub limit: usize,
    pub red_only: bool,
    pub pause: Option<&'a str>,
    /// Transient and deferred streaks, empty for a round that included no lane.
    pub strikes: BTreeMap<String, land::Streak>,
    pub spares: usize,
    pub rebuilds: u64,
    pub bisect_depth: u64,
}

/// Logs the round and returns its id, which the judgement of its candidate follows.
pub fn batch(round: &Round, included: &[(&PathBuf, &Ticket)], halve: Option<usize>) -> String {
    let case = BatchCase {
        base: round.base.to_owned(),
        tickets: round.tickets.iter().map(|(_, ticket)| land::Ticket { branch: ticket.branch.clone(), head: ticket.head.clone(), red_fix: ticket.red_fix.is_some() }).collect(),
        picks: round.built.picks.iter().map(candidate::recorded).collect(),
        limit: (round.limit != usize::MAX).then(|| count(round.limit)),
        red_only: round.red_only,
        pause: round.pause.map(str::to_owned),
        strikes: round.strikes.clone(),
        spares: count(round.spares),
        rebuilds: round.rebuilds,
        bisect_depth: round.bisect_depth,
        github: github_quota::seen(decision_log::now()),
    };
    let decision = BatchDecision { included: branches(included), halve: halve.map(count) };
    decision_log::record(Entry { kind: Kind::LandBatch, at: decision_log::now(), follows: None, case: &case, decision: &decision, decide_us: 0 })
}

pub struct Judged<'a> {
    pub verdict: &'a Verdict,
    pub included: &'a [(&'a PathBuf, &'a Ticket)],
    pub base_moved: bool,
    pub streak: u32,
    pub searched: bool,
}

pub fn judge(judged: &Judged, judgement: &Judgement, batch: &str, decide_us: u64) {
    let case = JudgeCase { verdict: verdict(judged.verdict), included: branches(judged.included), base_moved: judged.base_moved, streak: judged.streak, searched: judged.searched };
    let decision = recorded_judgement(judgement);
    decision_log::record(Entry { kind: Kind::LandJudge, at: decision_log::now(), follows: Some(batch), case: &case, decision: &decision, decide_us });
}

pub fn wave(wanted: usize, slots: Option<(usize, usize)>, width: usize) {
    let case = land::WaveCase { wanted: count(wanted), slots: slots.map(|(free, total)| (count(free), count(total))) };
    decision_log::record(Entry { kind: Kind::LandWave, at: decision_log::now(), follows: None, case: &case, decision: &land::WaveDecision { width: count(width) }, decide_us: 0 });
}

pub fn sheriff(case: &land::SheriffCase, step: &land::SheriffStep) {
    decision_log::record(Entry { kind: Kind::LandSheriff, at: decision_log::now(), follows: None, case, decision: step, decide_us: 0 });
}

pub(super) fn recorded_judgement(judgement: &Judgement) -> land::Judgement {
    match judgement {
        Judgement::Land => land::Judgement::Land,
        Judgement::Halve(half) => land::Judgement::Halve(count(*half)),
        Judgement::Culprit(detail) => land::Judgement::Culprit(detail.clone()),
        Judgement::Rebuild => land::Judgement::Rebuild,
        Judgement::RedMain(detail) => land::Judgement::RedMain(detail.clone()),
        Judgement::Hold(reason) => land::Judgement::Hold(reason.clone()),
        Judgement::Paused(detail) => land::Judgement::Paused(detail.clone()),
    }
}

fn verdict(verdict: &Verdict) -> land::Verdict {
    match verdict {
        Verdict::Green => land::Verdict::Green,
        Verdict::Refused(detail) => land::Verdict::Refused(detail.clone()),
        Verdict::RedMain(detail) => land::Verdict::RedMain(detail.clone()),
        Verdict::Transient(detail) => land::Verdict::Transient(detail.clone()),
        Verdict::BrokenBase(detail) => land::Verdict::BrokenBase(detail.clone()),
        Verdict::Deferred(detail) => land::Verdict::Deferred(detail.clone()),
    }
}

fn branches(included: &[(&PathBuf, &Ticket)]) -> Vec<String> {
    included.iter().map(|(_, ticket)| ticket.branch.clone()).collect()
}

fn count(number: usize) -> u64 {
    u64::try_from(number).unwrap_or(u64::MAX)
}
