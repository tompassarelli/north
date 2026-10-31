use crate::error::NorthResult;
use crate::fleet::model::SessionRecord;
use crate::fleet::store::{SessionRow, cache_sessions, cached_sessions};
use crate::foundation::clock::Timestamp;

use super::Fleet;

impl Fleet {
    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn cache_sessions(&self, rows: &[SessionRow]) -> NorthResult<()> {
        cache_sessions(&self.root, rows)
    }

    pub fn cached_sessions(&self) -> NorthResult<Option<Vec<SessionRow>>> {
        cached_sessions(&self.root)
    }

    pub(super) fn uncache_session(&self, name: &str) -> NorthResult<()> {
        match cached_sessions(&self.root)? {
            Some(rows) if rows.iter().any(|row| row.name == name) => cache_sessions(&self.root, &rows.into_iter().filter(|row| row.name != name).collect::<Vec<_>>()),
            _ => Ok(()),
        }
    }

    pub fn table_row(&self, record: &SessionRecord, now_ms: u64) -> SessionRow {
        SessionRow {
            name: record.name.clone(),
            host: self.host.clone(),
            status: self.status(record).label().to_owned(),
            created: iso(record.created_ms),
            updated: iso(now_ms),
            provider: record.provider.label().to_owned(),
            account: record.account.clone(),
            cwd: record.cwd.display().to_string(),
            parent: record.parent.clone(),
            placement: record.placement.clone(),
            machine: record.machine.as_ref().map(|machine| machine.name.clone()),
            tasks: record.tasks().map(str::to_owned).collect(),
        }
    }
}

fn iso(ms: u64) -> String {
    Timestamp::from_unix_millis(i64::try_from(ms).unwrap_or(i64::MAX)).iso()
}
