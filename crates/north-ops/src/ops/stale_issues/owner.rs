use std::collections::BTreeMap;
use std::path::Path;

use crate::threads::claims::ledger;

/// The session that last held each issue, from the newest claim on it, whole or by box.
pub struct Owners(BTreeMap<u64, String>);

impl Owners {
    pub fn read(database: &Path, name: &str) -> Result<Self, String> {
        if !database.is_file() {
            return Ok(Self(BTreeMap::new()));
        }
        let mut found = BTreeMap::new();
        for claim in ledger::claims(&ledger::open(database, false)?)? {
            let number = claim.item.strip_prefix(name).and_then(|rest| rest.strip_prefix('#')).and_then(|digits| digits.parse().ok());
            if let Some(number) = number {
                found.insert(number, claim.owner);
            }
        }
        Ok(Self(found))
    }

    pub fn of(&self, number: u64) -> Option<String> {
        self.0.get(&number).cloned()
    }
}
