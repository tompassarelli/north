//! The text of `north digest`: what landed, what is in flight, the open gates with their recommendation, and stalls.
use std::fmt::Write;

use crate::fleet::books::Landing;
use crate::fleet::model::Status;
use crate::fleet::store::{LAUNCH_GRACE_MS, SessionRow};
use crate::router::moment::Moment;

pub const STALL_AFTER_MS: u64 = 2 * 60 * 60 * 1000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GateLine {
    pub label: String,
    pub session: String,
    pub question: String,
    pub recommended: String,
    pub default_at_ms: Option<u64>,
}

pub struct Facts<'a> {
    pub now: Moment,
    pub now_ms: u64,
    pub since_label: &'a str,
    pub landed: &'a [Landing],
    pub sessions: &'a [SessionRow],
    pub gates: &'a [GateLine],
}

fn span(ms: u64) -> String {
    let minutes = ms / 60_000;
    match minutes {
        0..=59 => format!("{minutes}m"),
        _ => format!("{}h {:02}m", minutes / 60, minutes % 60),
    }
}

fn quiet_for(session: &SessionRow, now: Moment) -> Option<u64> {
    let seconds = now.seconds_since(Moment::parse(&session.updated)?);
    Some((seconds.max(0.0) * 1000.0) as u64)
}

fn stalled(session: &SessionRow, quiet_ms: u64) -> Option<String> {
    let limit = match session.status.as_str() {
        status if status == Status::Starting.label() => LAUNCH_GRACE_MS,
        status if status == Status::Working.label() => STALL_AFTER_MS,
        _ => return None,
    };
    (quiet_ms > limit).then(|| format!("{} ({}) {} with no update for {}", session.name, session.host, session.status, span(quiet_ms)))
}

pub fn render(facts: &Facts) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "Landed (last {}): {}", facts.since_label, facts.landed.len());
    for landing in facts.landed {
        let sha: String = landing.sha.chars().take(9).collect();
        let _ = writeln!(text, "  {} {sha} {} ago", landing.task_ref, span(facts.now_ms.saturating_sub(landing.landed_ms)));
    }
    let live: Vec<&SessionRow> = facts.sessions.iter().filter(|session| session.status != Status::Exited.label()).collect();
    let _ = writeln!(text, "In flight: {}", live.len());
    for session in &live {
        let tasks = if session.tasks.is_empty() { "no task".to_owned() } else { session.tasks.join(", ") };
        let _ = writeln!(text, "  {} ({}, {}) {}: {tasks}", session.name, session.host, session.provider, session.status);
    }
    let _ = writeln!(text, "Open gates: {}", facts.gates.len());
    for gate in facts.gates {
        let _ = writeln!(text, "  {} {}: {} Recommended: {}", gate.label, gate.session, gate.question, gate.recommended);
    }
    let mut stalls: Vec<String> = live.iter().filter_map(|session| stalled(session, quiet_for(session, facts.now)?)).collect();
    stalls.extend(facts.gates.iter().filter_map(|gate| gate.default_at_ms.filter(|at| *at < facts.now_ms).map(|at| format!("gate {} is past its default time by {}", gate.label, span(facts.now_ms - at)))));
    let _ = writeln!(text, "Stalls: {}", stalls.len());
    for stall in stalls {
        let _ = writeln!(text, "  {stall}");
    }
    text
}

