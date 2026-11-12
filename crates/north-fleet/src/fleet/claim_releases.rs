//! The sweep that gives up the claims exited sessions left in the ledger.
use std::collections::BTreeSet;

use crate::error::NorthResult;

use super::claims::{Claims, Rows};

/// What an exited sweep cleared, and the owners whose claim it could not give up, so they are swept again.
#[derive(Debug, Default)]
pub struct Swept {
    pub released: usize,
    pub unreleased: BTreeSet<String>,
}

/// Gives up every claim among `rows` whose owner is in `exited`, whatever its standing: a session that exited holds nothing, and the ledger keeps its row until someone clears it.
pub fn release_exited(claims: &dyn Claims, exited: &BTreeSet<String>, rows: Rows<'_>) -> NorthResult<Swept> {
    let mut swept = Swept::default();
    for (item, owner) in claims.holders(rows)? {
        if !exited.contains(&owner) {
            continue;
        }
        match claims.release(&item, &owner) {
            Ok(()) => swept.released += 1,
            Err(error) => {
                eprintln!("north fleet: {owner} did not release {item}: {}", error.user_message());
                swept.unreleased.insert(owner);
            }
        }
    }
    Ok(swept)
}
