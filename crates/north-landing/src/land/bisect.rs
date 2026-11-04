use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::after;
use super::checks::Verdict;
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
    base: String,
    streaks: BTreeMap<String, Streak>,
}

impl Strikes {
    pub fn load(queue: &Path, base: &str) -> Result<Self, String> {
        let path = queue.join(STRIKES);
        let stored = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Self>(&text).map_err(|error| format!("{}: {error}", path.display()))?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::default(),
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        Ok(if stored.base == base { stored } else { Self { base: base.to_owned(), streaks: BTreeMap::new() } })
    }

    pub fn save(&self, queue: &Path) -> Result<(), String> {
        let text = serde_json::to_string(self).map_err(|error| error.to_string())?;
        queue::write_atomic(&queue.join(STRIKES), &text).map_err(|error| error.to_string())
    }

    pub fn key(included: &[(&PathBuf, &Ticket)]) -> String {
        included.iter().map(|(path, ticket)| format!("{}@{}", path.file_name().unwrap_or_default().to_string_lossy(), ticket.head)).collect::<Vec<_>>().join(",")
    }

    pub fn exhausted(&self, key: &str) -> bool {
        self.streaks.get(key).is_some_and(|streak| streak.transient >= PERSISTENT_AFTER || streak.deferred >= PERSISTENT_AFTER)
    }

    pub fn record(&mut self, key: &str, verdict: &Verdict) -> u32 {
        match verdict {
            Verdict::Transient(_) => {
                let streak = self.streaks.entry(key.to_owned()).or_default();
                streak.transient += 1;
                streak.deferred = 0;
                streak.transient
            }
            Verdict::Deferred(_) => {
                let streak = self.streaks.entry(key.to_owned()).or_default();
                streak.deferred += 1;
                streak.transient = 0;
                streak.deferred
            }
            _ => {
                self.streaks.remove(key);
                0
            }
        }
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

fn refused(ticket: &Ticket, verdict: &str, log: Option<&Path>) -> String {
    let mut text = format!("north land: {} ({}) did not land: {verdict}", ticket.branch, ticket.head);
    if let Some(log) = log {
        let reason = refusal::reason(&fs::read_to_string(log).unwrap_or_default());
        if !reason.is_empty() {
            text.push('\n');
            text.push_str(&reason);
        }
        text.push_str(&format!("\nfull log: {}", log.display()));
    }
    text
}

pub fn notify(ticket: &Ticket, verdict: &str, log: Option<&Path>) {
    let Some(session) = ticket.session.as_deref().filter(|session| !session.is_empty()) else {
        return;
    };
    let text = match verdict.strip_prefix("landed ") {
        Some(sha) => format!("landed {} as {sha}", ticket.branch),
        None => refused(ticket, verdict, log),
    };
    if !after::send(session, &text) {
        eprintln!("north land: could not tell session {session} about {}", ticket.branch);
    }
}

pub fn decide(path: &Path, ticket: &Ticket, verdict: String, log: Option<&Path>) -> Result<(), String> {
    println!("north land: {} ({}): {verdict}", ticket.branch, ticket.head);
    queue::decide(path, ticket.clone(), verdict.clone()).map_err(|error| error.to_string())?;
    if rejected(&verdict) || verdict.starts_with("landed ") {
        notify(ticket, &verdict, log);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_persistent_transient_is_judged_like_a_refusal() {
        let log = Path::new("check.log");
        assert_eq!(escalate(Verdict::Transient("x".into()), 2, log), Verdict::Transient("x".into()));
        assert_eq!(escalate(Verdict::Deferred("x".into()), 2, log), Verdict::Deferred("x".into()));
        assert_eq!(escalate(Verdict::Transient("x".into()), 3, log), Verdict::Refused("persistent transient: check.log".into()));
        assert_eq!(escalate(Verdict::Deferred("x".into()), 4, log), Verdict::Refused("persistent deferred: check.log".into()));
        assert_eq!(escalate(Verdict::Green, 9, log), Verdict::Green);
        let mut strikes = Strikes::default();
        assert_eq!(strikes.record("k", &Verdict::Transient("x".into())), 1);
        assert_eq!(strikes.record("k", &Verdict::Transient("x".into())), 2);
        assert_eq!(strikes.record("k", &Verdict::Deferred("x".into())), 1);
        assert_eq!(strikes.record("k", &Verdict::Transient("x".into())), 1);
        assert!(!strikes.exhausted("k"));
        strikes.record("k", &Verdict::Transient("x".into()));
        strikes.record("k", &Verdict::Transient("x".into()));
        assert!(strikes.exhausted("k"));
        assert_eq!(strikes.record("k", &Verdict::Refused("r".into())), 0);
        assert!(!strikes.exhausted("k"));
    }

    #[test]
    fn halves_only_a_refusal_on_an_unmoved_base() {
        assert_eq!(judge(Verdict::Refused("log".into()), 5, false), Judgement::Halve(3));
        assert_eq!(judge(Verdict::Refused("log".into()), 2, false), Judgement::Halve(1));
        assert_eq!(judge(Verdict::Refused("log".into()), 1, false), Judgement::Culprit("log".into()));
        assert_eq!(judge(Verdict::Refused("log".into()), 3, true), Judgement::Rebuild);
        assert_eq!(judge(Verdict::Transient("fetch".into()), 3, false), Judgement::Hold("transient fetch".into()));
        assert_eq!(judge(Verdict::Deferred("DEFER".into()), 1, false), Judgement::Hold("deferred DEFER".into()));
        assert_eq!(judge(Verdict::RedMain("abc".into()), 1, false), Judgement::RedMain("abc".into()));
        assert_eq!(judge(Verdict::Green, 3, true), Judgement::Land);
    }
}
