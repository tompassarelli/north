use std::collections::BTreeSet;

use super::inventory::Entry;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    Archive,
    Pinned,
    Live,
    Unverifiable,
    Recent,
}

impl Verdict {
    pub(super) fn reason(&self) -> &'static str {
        match self {
            Self::Archive => "expired",
            Self::Pinned => "pinned",
            Self::Live => "held open by a running process",
            Self::Unverifiable => "holds a link or special file a copy cannot be verified against",
            Self::Recent => "within its retention age",
        }
    }
}

pub(super) fn judge(entry: &Entry, held: &BTreeSet<String>, now: u64, max_age_secs: u64) -> Verdict {
    if entry.pinned {
        Verdict::Pinned
    } else if held.contains(&entry.name) {
        Verdict::Live
    } else if entry.opaque {
        Verdict::Unverifiable
    } else if now.saturating_sub(entry.newest) < max_age_secs {
        Verdict::Recent
    } else {
        Verdict::Archive
    }
}
