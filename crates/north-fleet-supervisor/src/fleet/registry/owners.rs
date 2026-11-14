use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use super::Fleet;
use crate::fleet::model::{Phases, SessionRecord, Status};
use crate::fleet::records::{read_phased, record_file};
use crate::fleet::runtime::now_ms;
use crate::fleet::store::{SPOOL_DIR, SessionRow, cached_sessions};

/// How old the cached fleet table may be for a claim to read it; an older one may still show a session that exited long ago, so the registry is read instead.
const CACHE_TRUSTED_MS: u64 = 5 * 60 * 1000;

/// How long before the cache was written a record's change may predate the row the tick wrote for it, since a tick reads the registry before it writes the cache.
const TICK_SPAN_MS: u64 = 2 * 60 * 1000;

impl Fleet {
    /// The names, conversations and provider:conversation ids of the sessions that have not exited, by the fleet table the last tick cached, with this host's sessions whose record changed since read afresh, so a claim reads the same from every host and never reads the whole registry; `None` when the registry cannot be read, so an unread fleet is not taken for an empty one.
    pub fn live_owners(&self) -> Option<BTreeSet<String>> {
        let Some((rows, records)) = self.cached_view() else { return self.live_owners_read() };
        let mut owners = BTreeSet::new();
        self.live_records(&mut owners, &records);
        let now = now_ms();
        for row in rows.iter().filter(|row| row.status != Status::Exited.label() && !row.never_started(now)) {
            owner_ids(&mut owners, &row.name, row.conversation.as_deref(), &row.provider);
        }
        Some(owners)
    }

    fn live_owners_read(&self) -> Option<BTreeSet<String>> {
        let mut owners = BTreeSet::new();
        self.live_records(&mut owners, &self.sessions().ok()?);
        let elsewhere = cached_sessions(&self.root).ok().flatten().unwrap_or_default();
        for row in elsewhere.iter().filter(|row| row.host != self.host && row.status != Status::Exited.label() && !row.never_started(now_ms())) {
            owner_ids(&mut owners, &row.name, row.conversation.as_deref(), &row.provider);
        }
        Some(owners)
    }

    fn live_records(&self, owners: &mut BTreeSet<String>, records: &[SessionRecord]) {
        for record in records.iter().filter(|record| self.status(record) != Status::Exited && !super::claiming::never_started(record)) {
            owner_ids(owners, &record.name, record.conversation.as_deref(), record.provider.label());
        }
    }

    /// The fleet table the last tick cached, its rows of this host kept only for sessions still in the registry whose record has not changed since, and the records of this host's sessions no row names or changed since read afresh; `None` when no tick cached the table within [`CACHE_TRUSTED_MS`].
    fn cached_view(&self) -> Option<(Vec<SessionRow>, Vec<SessionRecord>)> {
        let cached_ms = self.sessions_cached_ms().filter(|at| now_ms().saturating_sub(*at) <= CACHE_TRUSTED_MS)?;
        let mut rows = cached_sessions(&self.root).ok().flatten()?;
        let names: BTreeSet<String> = fs::read_dir(&self.root).ok()?.flatten().filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir())).map(|entry| entry.file_name().to_string_lossy().into_owned()).filter(|name| !name.starts_with('.') && name != SPOOL_DIR).collect();
        let cached: BTreeSet<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        let since = cached_ms.saturating_sub(TICK_SPAN_MS);
        let changed: Vec<&String> = names.iter().filter(|name| !cached.contains(name.as_str()) || changed_since(&self.root.join(name), since)).collect();
        let mut phases = Phases::reading(&self.root).filter(|_| !changed.is_empty());
        let records: Vec<SessionRecord> = changed.into_iter().filter_map(|name| read_phased(phases.as_mut(), &self.root.join(name)).ok()).collect();
        let fresh: BTreeSet<&str> = records.iter().map(|record| record.name.as_str()).collect();
        rows.retain(|row| row.host != self.host || (names.contains(&row.name) && !fresh.contains(row.name.as_str())));
        Some((rows, records))
    }

    /// The sessions above a launch whose parent is `parent`: its parent, that one's parent and so on, read from the cached fleet table and the records of this host changed since, or the whole registry without a recent table.
    pub fn ancestors(&self, parent: Option<&str>) -> BTreeSet<String> {
        let (rows, records) = self.cached_view().unwrap_or_else(|| (cached_sessions(&self.root).ok().flatten().unwrap_or_default(), self.sessions().unwrap_or_default()));
        let mut parents: BTreeMap<String, String> = rows.into_iter().filter_map(|row| Some((row.name, row.parent?))).collect();
        parents.extend(records.into_iter().filter_map(|record| Some((record.name, record.parent?))));
        let mut found = BTreeSet::new();
        let mut next = parent.map(str::to_owned);
        while let Some(name) = next.filter(|name| !name.is_empty() && found.insert(name.clone())) {
            next = parents.get(&name).cloned();
        }
        found
    }
}

fn changed_since(session_dir: &Path, since_ms: u64) -> bool {
    let modified = fs::metadata(record_file(session_dir)).and_then(|metadata| metadata.modified()).ok().and_then(|at| at.duration_since(UNIX_EPOCH).ok());
    modified.is_none_or(|at| u64::try_from(at.as_millis()).unwrap_or(u64::MAX) >= since_ms)
}

fn owner_ids(owners: &mut BTreeSet<String>, name: &str, conversation: Option<&str>, provider: &str) {
    let conversation = conversation.filter(|conversation| !conversation.is_empty());
    owners.extend(Some(name).filter(|name| !name.is_empty()).map(str::to_owned));
    owners.extend(conversation.map(str::to_owned));
    if let Some(conversation) = conversation.filter(|_| !provider.is_empty()) {
        owners.insert(format!("{provider}:{conversation}"));
    }
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::fleet;
    use crate::fleet::store::{SessionRow, cache_sessions};

    #[test]
    fn a_session_live_on_another_host_owns_its_claims_here() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let row = |name: &str, host: &str, status: &str| SessionRow { name: name.into(), host: host.into(), status: status.into(), provider: "claude".into(), conversation: Some(format!("{name}-c")), ..SessionRow::default() };
        cache_sessions(&fleet.root, &[row("far", "other-host", "parked"), row("gone", "other-host", "exited"), row("mine", &fleet.host, "working")]).unwrap();

        let owners = fleet.live_owners().unwrap();

        assert_eq!(owners.into_iter().collect::<Vec<_>>(), ["claude:far-c", "far", "far-c"]);
    }

    /// On 2026-11-11 every box claim from the laptop read all 1,407 session directories of its registry, 7,035 record opens and each record's transcript, for 0.9 to 1.2 s of a 2 s bound.
    #[test]
    fn a_claim_reads_this_hosts_sessions_from_the_cached_table_and_only_records_changed_since() {
        let home = tempfile::tempdir().unwrap();
        let fleet = fleet(home.path());
        let unchanged = fleet.root.join("cached-worker");
        std::fs::create_dir_all(&unchanged).unwrap();
        std::fs::write(unchanged.join("session.json"), "a record no claim reads").unwrap();
        std::fs::File::options().write(true).open(unchanged.join("session.json")).unwrap().set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1)).unwrap();
        let _launched = super::super::fixtures::live(&fleet, "new-worker");
        let row = SessionRow { name: "cached-worker".into(), host: fleet.host.clone(), status: "working".into(), provider: "claude".into(), conversation: Some("cached-c".into()), ..SessionRow::default() };
        cache_sessions(&fleet.root, &[row]).unwrap();

        let owners = fleet.live_owners().unwrap();

        assert_eq!(owners.into_iter().collect::<Vec<_>>(), ["cached-c", "cached-worker", "claude:cached-c", "claude:new-worker", "new-worker"]);
    }
}
