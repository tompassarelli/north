use std::fmt;
use std::path::PathBuf;

use crate::book::Book;
use crate::facts::{Facts, LinkFacts, Place, PlaceFacts, QueueFacts, SlotHold};
use north_check_environment::Environment;
use north_lander::landing::check_timings::CEILING;

pub const OLDEST_QUEUED_SECS: u64 = 20 * 60;
pub const SINCE_LANDING_SECS: u64 = 30 * 60;
pub const ROUND_SILENCE_SECS: u64 = 15 * 60;
pub const LINK_LAG_SECS: u64 = 2 * 3600;
pub const BEHIND_LANDINGS: usize = 3;
pub const BEHIND_SECS: u64 = 15 * 60;
pub const REPAGE_SECS: u64 = 30 * 60;
pub const SLOT_GRACE_SECS: u64 = 900;
pub const SLOT_DEADLINE_SECS: u64 = CEILING + SLOT_GRACE_SECS;
pub const DISK_FREE_PERCENT: f64 = 10.0;
pub const PATIENCE_SECS: u64 = 5 * 60;
pub const NO_VERDICT_PROMOTIONS: u32 = 3;
pub const BASE_BROKEN_SECS: u64 = 10 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bound {
    OldestQueued,
    SinceLanding,
    RoundSilence,
    LinkLag,
    Behind,
    SlotHold,
    DiskFree,
    StaleWorktrees,
    InfrastructureFailures,
    UnservedQueues,
    OrphanQueues,
    SourceBuilds,
    NoVerdict,
    BaseBroken,
    Reach,
    AgentLink,
}

impl Bound {
    pub fn key(self) -> &'static str {
        match self {
            Bound::OldestQueued => "oldest-queued-lane",
            Bound::SinceLanding => "since-landing",
            Bound::RoundSilence => "round-silence",
            Bound::LinkLag => "link-lag",
            Bound::Behind => "link-behind",
            Bound::SlotHold => "slot-hold",
            Bound::DiskFree => "disk-free",
            Bound::StaleWorktrees => "stale-worktrees",
            Bound::InfrastructureFailures => "infrastructure-failures",
            Bound::UnservedQueues => "unserved-queues",
            Bound::OrphanQueues => "orphan-queues",
            Bound::SourceBuilds => "toolchain-from-source",
            Bound::NoVerdict => "promotion-no-verdict",
            Bound::BaseBroken => "base-broken",
            Bound::Reach => "lander-reach",
            Bound::AgentLink => "agent-link-stuck",
        }
    }

    pub fn limit(self) -> String {
        match self {
            Bound::OldestQueued => minutes(OLDEST_QUEUED_SECS),
            Bound::SinceLanding => minutes(SINCE_LANDING_SECS),
            Bound::RoundSilence => minutes(ROUND_SILENCE_SECS),
            Bound::LinkLag => minutes(LINK_LAG_SECS),
            Bound::Behind => format!("{BEHIND_LANDINGS} landings or {}", minutes(BEHIND_SECS)),
            Bound::BaseBroken => minutes(BASE_BROKEN_SECS),
            Bound::SlotHold => minutes(SLOT_DEADLINE_SECS),
            Bound::DiskFree => format!("{DISK_FREE_PERCENT:.0}% free"),
            Bound::StaleWorktrees | Bound::InfrastructureFailures | Bound::UnservedQueues | Bound::OrphanQueues | Bound::SourceBuilds | Bound::NoVerdict | Bound::Reach | Bound::AgentLink => "0".to_owned(),
        }
    }
}

pub fn minutes(seconds: u64) -> String {
    format!("{} min", seconds / 60)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Remedy {
    Prune { place: Place, repo: PathBuf },
    Release { place: Place, slot: String, pgid: u32 },
    Clear { place: Place, slot: String },
    Rerun { queue: PathBuf, ticket: PathBuf },
    RemoveQueue { queue: PathBuf },
    RemoveQueueAt { place: Place, queue: PathBuf },
    Evict { place: Place },
    Restart { unit: String },
    Rollback { home: PathBuf, from: String, to: String, units: Vec<String> },
    Alarm(String),
    Page(String),
    Promote,
    CheckHere { queue: PathBuf },
    Wait(String),
}

impl Remedy {
    pub fn class(&self) -> Option<&'static str> {
        match self {
            Remedy::Prune { .. } => Some("lander-stale-worktree"),
            Remedy::Release { .. } | Remedy::Clear { .. } => Some("lander-slot-overdue"),
            Remedy::Rerun { .. } => Some("lander-infrastructure-failure"),
            Remedy::RemoveQueue { .. } | Remedy::RemoveQueueAt { .. } => Some("lander-stray-queue"),
            Remedy::Evict { .. } => Some("lander-disk-pressure"),
            Remedy::Restart { .. } => Some("lander-wedged"),
            Remedy::Rollback { .. } => Some("lander-rollback"),
            Remedy::Alarm(_) => Some("lander-source-build"),
            Remedy::Page(_) => Some("lander-behind"),
            Remedy::Promote => Some("lander-promotion-retry"),
            Remedy::CheckHere { .. } => Some("lander-check-environment"),
            Remedy::Wait(_) => None,
        }
    }
}

impl fmt::Display for Remedy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Remedy::Prune { place, repo } => write!(formatter, "prune the missing worktree registrations of {} on {}", repo.display(), place.name),
            Remedy::Release { place, slot, pgid } => write!(formatter, "release slot {slot} on {} by ending its process group {pgid}", place.name),
            Remedy::Clear { place, slot } => write!(formatter, "remove the holder files of slot {slot} on {}, whose lock is free", place.name),
            Remedy::Rerun { ticket, .. } => write!(formatter, "requeue {} once", ticket.file_name().unwrap_or_default().to_string_lossy()),
            Remedy::RemoveQueue { queue } => write!(formatter, "remove the land queue {} no checkout serves and no ticket waits in", queue.display()),
            Remedy::RemoveQueueAt { place, queue } => write!(formatter, "remove the land queue {} on {}, which holds no ticket and no lander", queue.display(), place.name),
            Remedy::Evict { place } => write!(formatter, "evict unlocked offload caches on {}", place.name),
            Remedy::Restart { unit } => write!(formatter, "restart {unit}"),
            Remedy::Rollback { from, to, .. } => write!(formatter, "roll the lander link back from {from} to {to}, the last build that landed a batch green"),
            Remedy::Alarm(text) => write!(formatter, "tell the proxy: {text}"),
            Remedy::Page(text) => write!(formatter, "page the proxy and the lander's lead, again each {} while it lasts: {text}", minutes(REPAGE_SECS)),
            Remedy::Promote => write!(formatter, "retry the promotion with north runtime promote, which replays under lander-promote.lock"),
            Remedy::CheckHere { queue } => write!(formatter, "judge the candidates of {} in this host's check environment, where main last passed", queue.display()),
            Remedy::Wait(reason) => write!(formatter, "none: {reason}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Healthy,
    Known { name: &'static str, remedies: Vec<Remedy> },
    Unknown(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub bound: Bound,
    pub subject: String,
    pub value: String,
    pub over: bool,
    pub state: State,
}

impl Finding {
    pub fn key(&self) -> String {
        format!("{}:{}", self.bound.key(), self.subject)
    }

    fn healthy(bound: Bound, subject: &str, value: String) -> Self {
        Self { bound, subject: subject.to_owned(), value, over: false, state: State::Healthy }
    }

    fn over(bound: Bound, subject: &str, value: String, state: State) -> Self {
        Self { bound, subject: subject.to_owned(), value, over: true, state }
    }
}

fn known(name: &'static str, remedy: Remedy) -> State {
    State::Known { name, remedies: vec![remedy] }
}

fn wait(name: &'static str, reason: impl Into<String>) -> State {
    known(name, Remedy::Wait(reason.into()))
}

pub fn judge(facts: &Facts, book: &Book, now: u64) -> Vec<Finding> {
    let mut findings = vec![if facts.unserved.is_empty() {
        Finding::healthy(Bound::UnservedQueues, "lander", "0".to_owned())
    } else {
        Finding::over(Bound::UnservedQueues, "lander", facts.unserved.len().to_string(), State::Unknown(format!("lanes wait in queues no checkout on this host can serve: {}", facts.unserved.join("; "))))
    }];
    if facts.orphaned.is_empty() {
        findings.push(Finding::healthy(Bound::OrphanQueues, "lander", "0".to_owned()));
    }
    for queue in &facts.orphaned {
        let subject = format!("lander/{}", queue.file_name().unwrap_or_default().to_string_lossy());
        let remedy = Remedy::RemoveQueue { queue: queue.clone() };
        findings.push(Finding::over(Bound::OrphanQueues, &subject, "1".to_owned(), known("stray queue", remedy)));
    }
    for repository in facts.unreachable.iter().filter(|_| !facts.reach_deferred) {
        let detail = format!("the North Lander installation token cannot reach {repository}; add it to the North Lander App's installation");
        findings.push(Finding::over(Bound::Reach, repository, "unreachable".to_owned(), State::Unknown(detail)));
    }
    if facts.unreachable.is_empty() && !facts.reach_deferred {
        findings.push(match &facts.reach_error {
            Some(error) => Finding::over(Bound::Reach, "lander", "unchecked".to_owned(), State::Unknown(format!("could not ask GitHub whether a North Lander installation token reaches every repository: {error}"))),
            None => Finding::healthy(Bound::Reach, "lander", "0".to_owned()),
        });
    }
    for (place, queue) in &facts.orphaned_remote {
        let subject = format!("{}/{}", place.name, queue.file_name().unwrap_or_default().to_string_lossy());
        let remedy = Remedy::RemoveQueueAt { place: place.clone(), queue: queue.clone() };
        findings.push(Finding::over(Bound::OrphanQueues, &subject, "1".to_owned(), known("stray queue", remedy)));
    }
    for queue in &facts.queues {
        findings.extend(judge_queue(queue, facts.link.as_ref(), book, now));
    }
    if let Some(link) = &facts.link {
        findings.extend(judge_link(link));
    }
    findings.push(agent_link(facts.agent_link_stuck.as_deref()));
    for place in &facts.places {
        findings.extend(judge_place(place));
    }
    findings
}

fn judge_queue(queue: &QueueFacts, link: Option<&LinkFacts>, book: &Book, now: u64) -> Vec<Finding> {
    let subject = queue.name.as_str();
    let Some(oldest) = queue.oldest_queued else {
        let mut findings = vec![Finding::healthy(Bound::OldestQueued, subject, "none queued".to_owned()), Finding::healthy(Bound::SinceLanding, subject, "none queued".to_owned())];
        findings.extend(infrastructure(queue));
        findings.extend(source_builds(queue));
        findings.push(base_broken(queue));
        return findings;
    };
    let since_landing = queue.since_landing.map_or(oldest, |since| since.min(oldest));
    let deciding = queue.since_decision.is_some_and(|since| since.min(oldest) <= SINCE_LANDING_SECS);
    let silent = queue.round_silence.is_none_or(|silence| silence > ROUND_SILENCE_SECS);
    let mut findings = Vec::new();
    findings.push(if oldest <= OLDEST_QUEUED_SECS {
        Finding::healthy(Bound::OldestQueued, subject, minutes(oldest))
    } else if deciding {
        Finding::over(Bound::OldestQueued, subject, minutes(oldest), wait("backlog", "the lander decides lanes within its bound; this lane waits behind a backlog"))
    } else {
        Finding::over(Bound::OldestQueued, subject, minutes(oldest), wait("stalled", "the since-landing and round-silence bounds own a lander that decides nothing"))
    });
    findings.push(if since_landing <= SINCE_LANDING_SECS {
        Finding::healthy(Bound::SinceLanding, subject, minutes(since_landing))
    } else if silent {
        Finding::over(Bound::SinceLanding, subject, minutes(since_landing), wait("wedged", "the round-silence bound restarts the lander"))
    } else if let Some(reason) = &queue.paused {
        Finding::over(Bound::SinceLanding, subject, minutes(since_landing), wait("paused", format!("the queue is paused: {reason}")))
    } else if deciding {
        Finding::over(Bound::SinceLanding, subject, minutes(since_landing), wait("refusing", "the lander decides lanes within its bound; every recent one was refused"))
    } else {
        Finding::over(Bound::SinceLanding, subject, minutes(since_landing), State::Unknown(format!("the lander of {subject} runs rounds but has decided no lane for {}", minutes(since_landing))))
    });
    let silence = queue.round_silence.map_or_else(|| "no round recorded".to_owned(), minutes);
    findings.push(if silent { Finding::over(Bound::RoundSilence, subject, silence, wedged(queue, link, book, now)) } else { Finding::healthy(Bound::RoundSilence, subject, silence) });
    findings.extend(infrastructure(queue));
    findings.extend(source_builds(queue));
    findings.push(base_broken(queue));
    findings
}

fn wedged(queue: &QueueFacts, link: Option<&LinkFacts>, book: &Book, now: u64) -> State {
    if queue.unit_state.as_deref() == Some(crate::facts::MISSING_UNIT) {
        return State::Unknown(format!("lanes wait in the {} queue and no {} unit exists to serve it", queue.name, queue.unit));
    }
    if queue.unit_state.as_deref() == Some("inactive") {
        return wait("stopped", format!("{} is stopped on purpose; the doctor starts no stopped lander", queue.unit));
    }
    let key = format!("{}:{}", Bound::RoundSilence.key(), queue.name);
    let Some(restarted) = book.last_remedy(&key, "lander-wedged") else {
        return known("wedged", Remedy::Restart { unit: queue.unit.clone() });
    };
    let waited = now.saturating_sub(restarted);
    if waited < PATIENCE_SECS {
        return wait("wedged", format!("restarted {} ago; waiting for its first round", minutes(waited)));
    }
    match link.and_then(|link| rollback(link, book)) {
        Some(remedy) => known("wedged after a restart", remedy),
        None => State::Unknown(format!("{} stays silent {} after its restart, and no other lander build landed a batch green to roll back to", queue.unit, minutes(waited))),
    }
}

fn rollback(link: &LinkFacts, book: &Book) -> Option<Remedy> {
    if book.good.contains(&link.lander) {
        return None;
    }
    let to = book.good.iter().rev().find(|good| **good != link.lander && link.builds.contains(*good))?;
    Some(Remedy::Rollback { home: link.home.clone(), from: link.lander.clone(), to: to.clone(), units: link.units.clone() })
}

fn base_broken(queue: &QueueFacts) -> Finding {
    let subject = queue.name.as_str();
    let Some(broken) = queue.broken_for.filter(|broken| queue.paused.is_some() && *broken > BASE_BROKEN_SECS) else {
        return Finding::healthy(Bound::BaseBroken, subject, queue.broken_for.filter(|_| queue.paused.is_some()).map_or_else(|| "passes".to_owned(), minutes));
    };
    let state = match queue.environment {
        Some(Environment::Placed) => known("check environment broken", Remedy::CheckHere { queue: queue.queue.clone() }),
        _ => wait("check environment broken", "main fails its checks in the placed environment and on this host; the lander judges each candidate in both every round and resumes when either passes"),
    };
    Finding::over(Bound::BaseBroken, subject, minutes(broken), state)
}

fn infrastructure(queue: &QueueFacts) -> Vec<Finding> {
    if queue.infrastructure.is_empty() {
        return vec![Finding::healthy(Bound::InfrastructureFailures, &queue.name, "0".to_owned())];
    }
    queue
        .infrastructure
        .iter()
        .map(|failed| {
            let subject = format!("{}/{}", queue.name, failed.branch);
            let remedy = Remedy::Rerun { queue: queue.queue.clone(), ticket: failed.ticket.clone() };
            Finding::over(Bound::InfrastructureFailures, &subject, format!("1 ({})", failed.signature), known("infrastructure failure", remedy))
        })
        .collect()
}

fn source_builds(queue: &QueueFacts) -> Vec<Finding> {
    if queue.source_builds.is_empty() {
        return vec![Finding::healthy(Bound::SourceBuilds, &queue.name, "0".to_owned())];
    }
    queue
        .source_builds
        .iter()
        .map(|build| {
            let subject = format!("{}/{}", queue.name, build.log.file_name().unwrap_or_default().to_string_lossy());
            let cause = if build.offline {
                "its check line runs nix with --offline, which turns substitution off, so a host whose store lacks the closure builds it; keep that check on the host whose store holds the closure (land.local-only) or drop --offline"
            } else {
                "the check runs without substituters, on a nixpkgs pin the cache does not hold, or with an override that changes its hash"
            };
            let text = format!("a {} land check builds {} from source ({}), a toolchain the binary cache supplies: {cause}; it holds the queue's verdict for up to {}", queue.name, build.derivation, build.log.display(), minutes(SLOT_DEADLINE_SECS));
            Finding::over(Bound::SourceBuilds, &subject, format!("1 ({})", build.derivation), known("toolchain built from source", Remedy::Alarm(text)))
        })
        .collect()
}

fn judge_link(link: &LinkFacts) -> Vec<Finding> {
    let subject = "lander";
    let rolled_back = link.refused.as_deref() == Some(link.current.as_str());
    let behind = match &link.behind {
        None => Finding::healthy(Bound::Behind, subject, "on current".to_owned()),
        Some(behind) if behind.landings.is_none_or(|landings| landings <= BEHIND_LANDINGS) && behind.secs <= BEHIND_SECS => Finding::healthy(Bound::Behind, subject, behind.to_string()),
        Some(behind) if rolled_back => Finding::over(Bound::Behind, subject, behind.to_string(), known("rolled back", Remedy::Promote)),
        Some(behind) => {
            let text = format!("north land doctor: the lander link runs {}, {behind} behind current {}, so landings since are not live on the lander: {}", link.lander, link.current, behind.reason);
            Finding::over(Bound::Behind, subject, behind.to_string(), known("behind current", Remedy::Page(text)))
        }
    };
    let lag = match link.lag {
        None => Finding::healthy(Bound::LinkLag, subject, "unknown: no north checkout to read main from".to_owned()),
        Some(lag) if lag <= LINK_LAG_SECS => Finding::healthy(Bound::LinkLag, subject, minutes(lag)),
        Some(lag) if link.behind.is_some() =>Finding::over(Bound::LinkLag, subject, minutes(lag), wait("behind", "the link-behind bound owns a lander that does not follow current")),
        Some(lag) if let Some(hold) = &link.review_hold => Finding::over(Bound::LinkLag, subject, minutes(lag), wait("held for review", format!("the runtime stays on {} on purpose: {} holds commit {} for review, so no head containing it goes live: {}", link.lander, hold.session, hold.commit, hold.why))),
        Some(lag) => Finding::over(Bound::LinkLag, subject, minutes(lag), State::Unknown(format!("the lander runs {}, no older than current, yet main is {} newer: the runtime does not follow main", link.lander, minutes(lag)))),
    };
    vec![behind, lag, no_verdict(link)]
}

fn no_verdict(link: &LinkFacts) -> Finding {
    let Some(unreached) = link.unreached.as_ref().filter(|unreached| link.lander != link.current && link.current.ends_with(&unreached.candidate)) else {
        return Finding::healthy(Bound::NoVerdict, "lander", "0".to_owned());
    };
    let subject = format!("lander/{}#{}", unreached.candidate, unreached.attempts);
    let value = format!("{} promotion(s)", unreached.attempts);
    let state = if unreached.attempts < NO_VERDICT_PROMOTIONS {
        known("replay reached no verdict", Remedy::Promote)
    } else {
        State::Unknown(format!("{} promotions in a row of {} reached no verdict on a replay: {}", unreached.attempts, unreached.candidate, unreached.reason))
    };
    Finding::over(Bound::NoVerdict, &subject, value, state)
}

fn judge_place(place: &PlaceFacts) -> Vec<Finding> {
    let subject = place.place.name.as_str();
    let probe = match &place.probe {
        Ok(probe) => probe,
        Err(error) => return vec![Finding::over(Bound::DiskFree, subject, "unread".to_owned(), State::Unknown(format!("cannot read {subject}: {error}")))],
    };
    let mut findings = Vec::new();
    let free = format!("{:.0}% free ({:.0} GiB)", probe.free_percent, probe.free_gib);
    findings.push(if probe.free_percent >= DISK_FREE_PERCENT {
        Finding::healthy(Bound::DiskFree, subject, free)
    } else {
        Finding::over(Bound::DiskFree, subject, free, known("disk pressure", Remedy::Evict { place: place.place.clone() }))
    });
    let held: Vec<&SlotHold> = probe.slots.iter().filter(|slot| slot.locked || slot.held > SLOT_DEADLINE_SECS).collect();
    if held.is_empty() {
        findings.push(Finding::healthy(Bound::SlotHold, subject, "no slot held".to_owned()));
    }
    for slot in held {
        let slot_subject = format!("{subject}/{}", slot.slot);
        findings.push(if slot.held <= SLOT_DEADLINE_SECS {
            Finding::healthy(Bound::SlotHold, &slot_subject, minutes(slot.held))
        } else {
            let state = match slot.pgid {
                _ if !slot.locked => known("holder file outlives its holder", Remedy::Clear { place: place.place.clone(), slot: slot.slot.clone() }),
                Some(pgid) => known("slot past its deadline", Remedy::Release { place: place.place.clone(), slot: slot.slot.clone(), pgid }),
                None => State::Unknown(format!("slot {} on {subject} is held {} and names no process group to end", slot.slot, minutes(slot.held))),
            };
            Finding::over(Bound::SlotHold, &slot_subject, minutes(slot.held), state)
        });
    }
    if probe.prunable.is_empty() {
        findings.push(Finding::healthy(Bound::StaleWorktrees, subject, "0".to_owned()));
    }
    for (repo, count) in &probe.prunable {
        let remedy = Remedy::Prune { place: place.place.clone(), repo: repo.clone() };
        findings.push(Finding::over(Bound::StaleWorktrees, &format!("{subject}:{}", repo.display()), count.to_string(), known("missing registered worktree", remedy)));
    }
    findings
}

fn agent_link(stuck: Option<&str>) -> Finding {
    let Some(line) = stuck else { return Finding::healthy(Bound::AgentLink, "agent", "0".to_owned()) };
    let (runtime, why) = line.split_once(' ').unwrap_or((line, "no reason recorded"));
    Finding::over(Bound::AgentLink, "agent", "1".to_owned(), State::Unknown(format!("a rollback could not move the agent runtime link off {runtime}, the runtime that failed its promotion, so agent sessions still start on it: {why}; the next runtime update that moves the link clears this")))
}
