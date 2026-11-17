use super::after::cleanup;
use super::lander::Lander;
use super::queue::Ticket;

pub enum Decision<'a> {
    Landed { sha: &'a str },
    Refused { verdict: &'a str },
}

pub fn settle(lander: &Lander, ticket: &Ticket, decision: &Decision) {
    let landed = match decision {
        Decision::Landed { sha } => Some(*sha),
        Decision::Refused { verdict } => verdict.strip_prefix("landed ").and_then(|rest| rest.split_whitespace().next()),
    };
    if let Some(landed) = landed {
        cleanup::origin_branches(lander, ticket, landed);
    }
}
