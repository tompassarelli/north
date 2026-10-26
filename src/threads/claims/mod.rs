//! When a NorthThreads claim still holds its issue: the one rule `threads claim`, `threads list`
//! and dispatch's survey share, so a worker is never refused by a claim dispatch saw as free.
pub(crate) mod issues;
pub(crate) mod ledger;
pub(crate) mod verbs;
#[cfg(test)]
mod tests;

/// How long past its ETA, with nothing from its owner, a claim holds before anyone may take it.
pub(crate) const STALE_AFTER_S: i64 = 24 * 3600;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Claim {
    pub item: String,
    pub owner: String,
    pub since: i64,
    pub eta_min: Option<i64>,
    /// The latest event the owner recorded on the item.
    pub owner_seen: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Issue {
    Open { opened: i64 },
    Closed,
    Missing,
    /// GitHub or the native item table could not be read, so only the ETA rule applies.
    Unread,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Standing {
    Live,
    /// Past its ETA by [`STALE_AFTER_S`] with no newer owner event since `idle_since`.
    Stale { idle_since: i64 },
    Void(&'static str),
}

impl Claim {
    pub(crate) fn standing(&self, issue: Issue, now: i64) -> Standing {
        match issue {
            Issue::Missing => return Standing::Void("its issue does not exist"),
            Issue::Closed => return Standing::Void("its issue is closed"),
            Issue::Open { opened } if self.since < opened => return Standing::Void("it was made before its issue was opened"),
            Issue::Open { .. } | Issue::Unread => {}
        }
        let due = self.since + self.eta_min.unwrap_or(0) * 60;
        let idle_since = self.owner_seen.map_or(due, |seen| seen.max(due));
        if now - idle_since > STALE_AFTER_S { Standing::Stale { idle_since } } else { Standing::Live }
    }
}
