//! The fleet's read-only views: each reads the registry, the ledger or the fleet table and
//! changes nothing, so a new one lands here and leaves the router's command dispatch alone.
use crate::error::NorthResult;
use crate::fleet::registry::Fleet;

use super::{drafts, hops, list, rebooted, reports, spend, tree, withheld};

/// The view `words` names, or `None` when they name no read-only view.
pub(super) fn run(fleet: &Fleet, words: &[&str]) -> Option<NorthResult<()>> {
    let [verb, rest @ ..] = words else { return None };
    Some(match (*verb, rest) {
        (verb @ ("list" | "stranded"), rest) => list::list(fleet, verb, rest),
        ("rebooted", rest) => rebooted::run(fleet, rest),
        ("reports", rest) => reports::run(fleet, rest),
        ("held", rest) => withheld::run(fleet, rest),
        ("drafts", rest) => drafts::run(rest),
        ("tree", []) => tree::tree(fleet),
        ("hops", names) => hops::run(fleet, names),
        ("spend", rest) => spend::run(fleet, rest),
        _ => return None,
    })
}
