use std::path::Path;

use super::after;
use super::checks::Verdict;
use super::queue::{self, Ticket};

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

fn rejected(verdict: &str) -> bool {
    verdict.starts_with("failed ") || verdict.starts_with("conflict ")
}

pub fn notify(ticket: &Ticket, verdict: &str) {
    let Some(session) = ticket.session.as_deref().filter(|session| !session.is_empty()) else {
        return;
    };
    let text = match verdict.strip_prefix("landed ") {
        Some(sha) => format!("landed {} as {sha}", ticket.branch),
        None => format!("north land: {} ({}) did not land: {verdict}", ticket.branch, ticket.head),
    };
    if !after::send(session, &text) {
        eprintln!("north land: could not tell session {session} about {}", ticket.branch);
    }
}

pub fn decide(path: &Path, ticket: &Ticket, verdict: String) -> Result<(), String> {
    println!("north land: {} ({}): {verdict}", ticket.branch, ticket.head);
    queue::decide(path, ticket.clone(), verdict.clone()).map_err(|error| error.to_string())?;
    if rejected(&verdict) || verdict.starts_with("landed ") {
        notify(ticket, &verdict);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
