//! The root filesystem's floor for heavy builds and runtime canaries, with the reserve each running build
//! keeps for its target.

use super::{Class, Facts, default_mib};

/// A heavy build is deferred while it would take the root filesystem under the floor; an unread disk admits.
pub(super) fn disk_short(facts: &Facts) -> bool {
    matches!(facts.class, Class::Heavy | Class::Runtime) && disk_below_floor(facts)
}

/// Whether the free space, less a target's reserve for every heavy or runtime build already running and for
/// this one, is under the floor: builds admitted above it still write their targets afterwards.
pub(super) fn disk_below_floor(facts: &Facts) -> bool {
    let builds = facts.leases.iter().filter(|lease| matches!(lease.class.as_str(), "heavy" | "runtime")).count() + 1;
    let reserved = builds as f64 * default_mib("build_disk_reserve_gib");
    facts.disk_free_gib.is_some_and(|free| free - reserved < default_mib("build_disk_free_gib"))
}
