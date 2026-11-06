use std::fmt;
use std::path::PathBuf;

use crate::book::Book;
use crate::facts::{Facts, LinkFacts, Place, PlaceFacts, QueueFacts};
use north_landing::landing::check_timings::CEILING;

pub const OLDEST_QUEUED_SECS: u64 = 20 * 60;
pub const SINCE_LANDING_SECS: u64 = 30 * 60;
pub const ROUND_SILENCE_SECS: u64 = 15 * 60;
pub const LINK_LAG_SECS: u64 = 2 * 3600;
pub const HELD_SECS: u64 = 15 * 60;
pub const SLOT_GRACE_SECS: u64 = 900;
pub const SLOT_DEADLINE_SECS: u64 = CEILING + SLOT_GRACE_SECS;
pub const DISK_FREE_PERCENT: f64 = 10.0;
pub const PATIENCE_SECS: u64 = 5 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bound {
    OldestQueued,
    SinceLanding,
    RoundSilence,
    LinkLag,
    Held,
    SlotHold,
    DiskFree,
    StaleWorktrees,
    InfrastructureFailures,
    UnservedQueues,
    SourceBuilds,
}

impl Bound {
    pub fn key(self) -> &'static str {
        match self {
            Bound::OldestQueued => "oldest-queued-lane",
            Bound::SinceLanding => "since-landing",
            Bound::RoundSilence => "round-silence",
            Bound::LinkLag => "link-lag",
            Bound::Held => "link-held",
            Bound::SlotHold => "slot-hold",
            Bound::DiskFree => "disk-free",
            Bound::StaleWorktrees => "stale-worktrees",
            Bound::InfrastructureFailures => "infrastructure-failures",
            Bound::UnservedQueues => "unserved-queues",
            Bound::SourceBuilds => "toolchain-from-source",
        }
    }

    pub fn limit(self) -> String {
        match self {
            Bound::OldestQueued => minutes(OLDEST_QUEUED_SECS),
            Bound::SinceLanding => minutes(SINCE_LANDING_SECS),
            Bound::RoundSilence => minutes(ROUND_SILENCE_SECS),
            Bound::LinkLag => minutes(LINK_LAG_SECS),
            Bound::Held => minutes(HELD_SECS),
            Bound::SlotHold => minutes(SLOT_DEADLINE_SECS),
            Bound::DiskFree => format!("{DISK_FREE_PERCENT:.0}% free"),
            Bound::StaleWorktrees | Bound::InfrastructureFailures | Bound::UnservedQueues | Bound::SourceBuilds => "0".to_owned(),
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
    Rerun { queue: PathBuf, ticket: PathBuf },
    Evict { place: Place },
    Restart { unit: String },
    Rollback { home: PathBuf, from: String, to: String, units: Vec<String> },
    Alarm(String),
    Wait(String),
}

impl Remedy {
    pub fn class(&self) -> Option<&'static str> {
        match self {
            Remedy::Prune { .. } => Some("lander-stale-worktree"),
            Remedy::Release { .. } => Some("lander-slot-overdue"),
            Remedy::Rerun { .. } => Some("lander-infrastructure-failure"),
            Remedy::Evict { .. } => Some("lander-disk-pressure"),
            Remedy::Restart { .. } => Some("lander-wedged"),
            Remedy::Rollback { .. } => Some("lander-rollback"),
            Remedy::Alarm(_) => Some("lander-source-build"),
            Remedy::Wait(_) => None,
        }
    }
}

impl fmt::Display for Remedy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Remedy::Prune { place, repo } => write!(formatter, "prune the missing worktree registrations of {} on {}", repo.display(), place.name),
            Remedy::Release { place, slot, pgid } => write!(formatter, "release slot {slot} on {} by ending its process group {pgid}", place.name),
            Remedy::Rerun { ticket, .. } => write!(formatter, "requeue {} once", ticket.file_name().unwrap_or_default().to_string_lossy()),
            Remedy::Evict { place } => write!(formatter, "evict unlocked offload caches on {}", place.name),
            Remedy::Restart { unit } => write!(formatter, "restart {unit}"),
            Remedy::Rollback { from, to, .. } => write!(formatter, "roll the lander link back from {from} to {to}, the last build that landed a batch green"),
            Remedy::Alarm(text) => write!(formatter, "tell the proxy: {text}"),
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
    for queue in &facts.queues {
        findings.extend(judge_queue(queue, facts.link.as_ref(), book, now));
    }
    if let Some(link) = &facts.link {
        findings.extend(judge_link(link));
    }
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
    let held = link.held.unwrap_or(0);
    let rolled_back = link.refused.as_deref() == Some(link.current.as_str());
    let held = if held <= HELD_SECS {
        Finding::healthy(Bound::Held, subject, minutes(held))
    } else if rolled_back {
        Finding::over(Bound::Held, subject, minutes(held), wait("rolled back", format!("the doctor rolled the lander back from {}; north land promote refuses it until current moves on", link.current)))
    } else {
        Finding::over(Bound::Held, subject, minutes(held), wait("held", "north land promote records the hold and alarms the proxy once per hold"))
    };
    let lag = match link.lag {
        None => Finding::healthy(Bound::LinkLag, subject, "unknown: no north checkout to read main from".to_owned()),
        Some(lag) if lag <= LINK_LAG_SECS => Finding::healthy(Bound::LinkLag, subject, minutes(lag)),
        Some(lag) if link.lander != link.current => Finding::over(Bound::LinkLag, subject, minutes(lag), wait("held", "the link-held bound owns a lander that does not follow current")),
        Some(lag) => Finding::over(Bound::LinkLag, subject, minutes(lag), State::Unknown(format!("the lander runs {} and current is the same, yet main is {} newer: the runtime does not follow main", link.lander, minutes(lag)))),
    };
    vec![held, lag]
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
    if probe.slots.is_empty() {
        findings.push(Finding::healthy(Bound::SlotHold, subject, "no slot held".to_owned()));
    }
    for slot in &probe.slots {
        let slot_subject = format!("{subject}/{}", slot.slot);
        findings.push(if slot.held <= SLOT_DEADLINE_SECS {
            Finding::healthy(Bound::SlotHold, &slot_subject, minutes(slot.held))
        } else {
            let state = match slot.pgid {
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
