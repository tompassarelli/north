use std::collections::BTreeSet;

use super::Fleet;
use crate::fleet::model::Status;

impl Fleet {
    /// The names, conversations and provider:conversation ids of this host's sessions that have not exited; `None` when the registry cannot be read, so an unread fleet is not taken for an empty one.
    pub fn live_owners(&self) -> Option<BTreeSet<String>> {
        let mut owners = BTreeSet::new();
        for record in self.sessions().ok()?.iter().filter(|record| self.status(record) != Status::Exited) {
            let conversation = record.conversation.as_deref().filter(|conversation| !conversation.is_empty());
            owners.extend(Some(record.name.as_str()).filter(|name| !name.is_empty()).map(str::to_owned));
            owners.extend(conversation.map(str::to_owned));
            if let Some(conversation) = conversation {
                owners.insert(format!("{}:{conversation}", record.provider.label()));
            }
        }
        Some(owners)
    }
}
