use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::landing::check_findings;
use crate::platform::attribution::Cause;
pub use crate::platform::land_notice::OWNER;
use crate::platform::land_notice::{self, Notice};
use super::after;
use super::checks::Verdict;
use super::incidents;
use super::queue::{self, Ticket};
use super::refusal;

pub const PERSISTENT_AFTER: u32 = 3;

const STRIKES: &str = "strikes";

#[derive(Debug, PartialEq, Eq)]
pub enum Judgement {
    Land,
    Halve(usize),
    Culprit(String),
    Rebuild,
    RedMain(String),
    Hold(String),
    Paused(String),
}

pub fn half(included: usize) -> usize {
    included.div_ceil(2)
}

pub fn judge(verdict: Verdict, included: usize, base_moved: bool) -> Judgement {
    match verdict {
        Verdict::Green => Judgement::Land,
        _ if base_moved => Judgement::Rebuild,
        Verdict::Refused(_) if included > 1 => Judgement::Halve(half(included)),
        Verdict::Refused(detail) => Judgement::Culprit(detail),
        Verdict::RedMain(detail) => Judgement::RedMain(detail),
        Verdict::Transient(detail) => Judgement::Hold(format!("transient {detail}")),
        Verdict::BrokenBase(detail) => Judgement::Paused(detail),
        Verdict::Deferred(detail) => Judgement::Hold(format!("deferred {detail}")),
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Streak {
    transient: u32,
    deferred: u32,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Strikes {
    streaks: BTreeMap<String, Streak>,
}

impl Strikes {
    pub fn load(queue: &Path, lanes: &[String]) -> Result<Self, String> {
        let path = queue.join(STRIKES);
        let mut stored = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Self>(&text).map_err(|error| format!("{}: {error}", path.display()))?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        stored.streaks.retain(|lane, _| lanes.contains(lane));
        Ok(stored)
    }

    pub fn save(&self, queue: &Path) -> Result<(), String> {
        let text = serde_json::to_string(self).map_err(|error| error.to_string())?;
        queue::write_atomic(&queue.join(STRIKES), &text).map_err(|error| error.to_string())
    }

    pub fn lanes(included: &[(&PathBuf, &Ticket)]) -> Vec<String> {
        included.iter().map(|(path, ticket)| format!("{}@{}", path.file_name().unwrap_or_default().to_string_lossy(), ticket.head)).collect()
    }

    /// The streaks as the decision log records them.
    pub fn streaks(&self) -> BTreeMap<String, north_strategy_types::land::Streak> {
        self.streaks.iter().map(|(key, streak)| (key.clone(), north_strategy_types::land::Streak { transient: streak.transient, deferred: streak.deferred })).collect()
    }

    pub fn exhausted(&self, lanes: &[String]) -> bool {
        lanes.iter().any(|lane| self.streaks.get(lane).is_some_and(|streak| streak.transient >= PERSISTENT_AFTER || streak.deferred >= PERSISTENT_AFTER))
    }

    pub fn record(&mut self, lanes: &[String], verdict: &Verdict) -> u32 {
        let transient = match verdict {
            Verdict::Transient(_) => true,
            Verdict::Deferred(_) => false,
            _ => {
                for lane in lanes {
                    self.streaks.remove(lane);
                }
                return 0;
            }
        };
        lanes
            .iter()
            .map(|lane| {
                let streak = self.streaks.entry(lane.clone()).or_default();
                if transient {
                    streak.transient += 1;
                    streak.deferred = 0;
                    streak.transient
                } else {
                    streak.deferred += 1;
                    streak.transient = 0;
                    streak.deferred
                }
            })
            .max()
            .unwrap_or(0)
    }
}

pub fn escalate(verdict: Verdict, streak: u32, log: &Path) -> Verdict {
    match verdict {
        Verdict::Transient(_) if streak >= PERSISTENT_AFTER => Verdict::Refused(format!("persistent transient: {}", log.display())),
        Verdict::Deferred(_) if streak >= PERSISTENT_AFTER => Verdict::Refused(format!("persistent deferred: {}", log.display())),
        other => other,
    }
}

fn rejected(verdict: &str) -> bool {
    verdict.starts_with("failed ") || verdict.starts_with("conflict ")
}

/// A refusal by the lane's own failing checks, whose error count dispatch compares across the lane's tries.
fn own_failure(verdict: &str) -> bool {
    verdict.starts_with("failed ") && incidents::blame(verdict) == Some(Cause::Worker)
}

fn refused(ticket: &Ticket, verdict: &str, log: Option<&Path>) -> String {
    let read = log.and_then(|log| fs::read_to_string(log).ok());
    let errors = read.as_deref().filter(|_| own_failure(verdict)).map(check_findings::errors);
    let mut text = Notice::Refused { branch: ticket.branch.clone(), head: ticket.head.clone(), verdict: verdict.to_owned(), errors }.text();
    if let Some(log) = log {
        let reason = refusal::reason(read.as_deref().unwrap_or_default());
        if !reason.is_empty() {
            text.push('\n');
            text.push_str(&reason);
        }
        text.push_str(&format!("\nfull log: {}", log.display()));
    }
    text
}

pub fn notify(ticket: &Ticket, verdict: &str, log: Option<&Path>) {
    let sessions = land_notice::told(ticket.session.as_deref(), verdict);
    let Some(first) = sessions.first() else { return };
    let text = match verdict.strip_prefix("landed ") {
        Some(sha) => Notice::Landed { branch: ticket.branch.clone(), sha: sha.to_owned() }.text(),
        None => refused(ticket, verdict, log),
    };
    if !sessions.iter().any(|session| after::send(session, &text)) {
        eprintln!("north land: could not tell session {first} about {}", ticket.branch);
    }
}

pub fn decide(path: &Path, ticket: &Ticket, verdict: String, log: Option<&Path>, main: &str) -> Result<(), String> {
    println!("north land: {} ({}): {verdict}", ticket.branch, ticket.head);
    queue::decide(path, Ticket { decided_on: Some(main.to_owned()), ..ticket.clone() }, verdict.clone()).map_err(|error| error.to_string())?;
    if rejected(&verdict) || verdict.starts_with("landed ") {
        notify(ticket, &verdict, log);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lane_waiting_on_a_main_that_fails_its_own_check_is_never_refused() {
        let log = Path::new("check.log");
        let tail = "safe-push: verdict transient main's check environment is broken: cargo clippy on gex45: error[E0063]: missing field `breaches`";
        let broken = super::super::checks::parse(tail).unwrap();
        assert!(matches!(broken, Verdict::BrokenBase(_)));
        let (mut strikes, lane) = (Strikes::default(), vec!["k".to_owned()]);
        for _ in 0..PERSISTENT_AFTER * 3 {
            for verdict in [broken.clone(), Verdict::RedMain("abc".into())] {
                let streak = strikes.record(&lane, &verdict);
                assert_eq!(escalate(verdict.clone(), streak, log), verdict);
            }
        }
        assert!(!strikes.exhausted(&lane));
    }

}
