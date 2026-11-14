use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::threads::claims::ledger;

static OWNER_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^Owner:\s*([A-Za-z][A-Za-z0-9_-]*)(?:[^#A-Za-z0-9_-]|$)").expect("owner pattern"));

fn declared(body: &str) -> Option<String> {
    OWNER_LINE.captures(body).map(|line| line[1].to_owned())
}

/// The session that last held each issue, from the newest claim on it, whole or by box.
#[derive(Default)]
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

    /// The newest claim's session, else the one an `Owner: SESSION` line of the issue's `body` names.
    pub fn of(&self, number: u64, body: &str) -> Option<String> {
        self.0.get(&number).cloned().or_else(|| declared(body))
    }
}
