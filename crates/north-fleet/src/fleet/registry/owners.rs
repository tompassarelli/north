use std::collections::BTreeSet;

use super::Fleet;
use crate::fleet::model::Status;
use crate::fleet::store::cached_sessions;

impl Fleet {
    /// The names, conversations and provider:conversation ids of the sessions that have not exited, on this host by its registry and on every other host by the cached fleet table, so a claim reads the same from every host; `None` when the registry cannot be read, so an unread fleet is not taken for an empty one.
    pub fn live_owners(&self) -> Option<BTreeSet<String>> {
        let mut owners = BTreeSet::new();
        for record in self.sessions().ok()?.iter().filter(|record| self.status(record) != Status::Exited) {
            owner_ids(&mut owners, &record.name, record.conversation.as_deref(), record.provider.label());
        }
        let elsewhere = cached_sessions(&self.root).ok().flatten().unwrap_or_default();
        for row in elsewhere.iter().filter(|row| row.host != self.host && row.status != Status::Exited.label()) {
            owner_ids(&mut owners, &row.name, row.conversation.as_deref(), &row.provider);
        }
        Some(owners)
    }
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
}
