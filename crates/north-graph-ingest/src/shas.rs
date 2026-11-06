use rusqlite::{Connection, OpenFlags};

use crate::places::Places;

pub struct Shas(Vec<String>);

impl Shas {
    pub fn read(places: &Places) -> Self {
        let mut known: Vec<String> = Vec::new();
        if let Ok(connection) = Connection::open_with_flags(&places.ledger, OpenFlags::SQLITE_OPEN_READ_ONLY)
            && let Ok(mut statement) = connection.prepare("SELECT head_sha FROM checks WHERE head_sha IS NOT NULL UNION SELECT landed_sha FROM tasks WHERE landed_sha IS NOT NULL")
            && let Ok(rows) = statement.query_map([], |row| row.get::<_, String>(0))
        {
            known.extend(rows.flatten());
        }
        for push in north_fleet::fleet::landings::pushes(&places.pushes) {
            known.extend([push.oid, push.landed]);
        }
        known.retain(|sha| !sha.is_empty());
        known.iter_mut().for_each(|sha| sha.make_ascii_lowercase());
        known.sort_unstable();
        known.dedup();
        Self(known)
    }

    pub fn resolve(&self, prefix: &str) -> String {
        let prefix = prefix.to_ascii_lowercase();
        let start = self.0.partition_point(|sha| sha.as_str() < prefix.as_str());
        let mut matches = self.0[start..].iter().take_while(|sha| sha.starts_with(&prefix));
        match (matches.next(), matches.next()) {
            (Some(only), None) => only.clone(),
            _ => prefix,
        }
    }
}
