use super::admission::{AheadTicket, Class, Facts, Reason, batch_fits, decide, default_mib, host_holds};

pub(super) fn queued_ahead(facts: &Facts) -> u32 {
    if host_holds(facts, facts.gaming_class.unwrap_or(facts.class)) {
        return facts.queued_ahead;
    }
    let passed = facts.queued_ahead_tickets.iter().filter(|ticket| ticket.requested_class.is_some_and(|class| host_holds(facts, class)) || backfilled(facts, ticket)).count();
    facts.queued_ahead.saturating_sub(u32::try_from(passed).unwrap_or(u32::MAX))
}

/// A ticket ahead that the host would not admit on its own now holds no place for a request behind it until it has
/// waited `backfill_seconds`; from then everything behind it waits, so the room frees for it before any later arrival.
fn backfilled(facts: &Facts, ticket: &AheadTicket) -> bool {
    ticket.queued_seconds < default_mib("backfill_seconds") && cannot_start(facts, ticket)
}

fn cannot_start(facts: &Facts, ticket: &AheadTicket) -> bool {
    let Some(cpus) = ticket.requested_cpus else { return false };
    if !batch_fits(facts, cpus) {
        return true;
    }
    let Some(class @ (Class::Moderate | Class::Heavy)) = ticket.requested_class else { return false };
    decide(&alone(facts, ticket, class, cpus)) != Reason::Run
}

fn alone(facts: &Facts, ticket: &AheadTicket, class: Class, cpus: f64) -> Facts {
    Facts {
        class,
        gaming_class: None,
        requested_cpus: cpus,
        requested_memory_mib: ticket.requested_memory_mib.unwrap_or(facts.requested_memory_mib),
        queued_ahead: 0,
        queued_ahead_tickets: Vec::new(),
        queued_ahead_new_clients: Vec::new(),
        new_clients: 0,
        new_desktops: 0,
        alone: false,
        owner: None,
        pair: None,
        quiet: false,
        beside_native: false,
        before_drain: false,
        ticket: None,
        requester_lease: None,
        need: None,
        holds_offload_slot: false,
        ..facts.clone()
    }
}
