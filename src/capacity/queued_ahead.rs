use super::admission::{AheadTicket, Facts, batch_fits, default_mib, host_holds};

pub(super) fn queued_ahead(facts: &Facts) -> u32 {
    if host_holds(facts, facts.gaming_class.unwrap_or(facts.class)) {
        return facts.queued_ahead;
    }
    let passed = facts.queued_ahead_tickets.iter().filter(|ticket| ticket.requested_class.is_some_and(|class| host_holds(facts, class)) || backfilled(facts, ticket)).count();
    facts.queued_ahead.saturating_sub(u32::try_from(passed).unwrap_or(u32::MAX))
}

/// A ticket ahead that the free batch share cannot fit holds no place for a smaller one behind it until it has waited
/// `backfill_seconds`; from then everything behind it waits, so the share frees for it before any later arrival.
fn backfilled(facts: &Facts, ticket: &AheadTicket) -> bool {
    ticket.queued_seconds < default_mib("backfill_seconds") && ticket.requested_cpus.is_some_and(|cpus| !batch_fits(facts, cpus))
}
