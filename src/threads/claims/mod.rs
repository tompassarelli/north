//! When a NorthThreads claim still holds its issue: the one rule `threads claim`, `threads list`
//! and dispatch's survey share, so a worker is never refused by a claim dispatch saw as free.
use std::collections::BTreeSet;
use std::sync::LazyLock;

use toml_edit::DocumentMut;

pub(crate) mod issues;
pub(crate) mod ledger;
pub(crate) mod verbs;
#[cfg(test)]
mod tests;

pub(crate) static OVERDUE_RATIO: LazyLock<f64> = LazyLock::new(|| {
    let defaults: DocumentMut = include_str!("../../../harness/defaults.toml").parse().expect("harness/defaults.toml parses");
    defaults["overdue_ratio"]["value"].as_integer().map(|ratio| ratio as f64).or_else(|| defaults["overdue_ratio"]["value"].as_float()).expect("overdue_ratio is a number")
});

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
    /// Past its ETA by [`STALE_AFTER_S`], or past [`OVERDUE_RATIO`] times its ETA, with no newer owner event since `idle_since`.
    Stale { idle_since: i64 },
    Void(&'static str),
}

impl Claim {
    /// `alive` is the owners of the sessions alive now, or `None` when no fleet could be read, so only the issue and ETA rules apply.
    pub(crate) fn standing(&self, issue: Issue, alive: Option<&BTreeSet<String>>, now: i64) -> Standing {
        match issue {
            Issue::Missing => return Standing::Void("its issue does not exist"),
            Issue::Closed => return Standing::Void("its issue is closed"),
            Issue::Open { opened } if self.since < opened => return Standing::Void("it was made before its issue was opened"),
            Issue::Open { .. } | Issue::Unread => {}
        }
        if alive.is_some_and(|alive| !alive.contains(&self.owner)) {
            return Standing::Void("its owner is not a live fleet session");
        }
        let due = self.since + self.eta_min.unwrap_or(0) * 60;
        let idle_since = self.owner_seen.map_or(due, |seen| seen.max(due));
        let overdue = self.eta_min.is_some_and(|eta| now - self.since > (*OVERDUE_RATIO * eta as f64 * 60.0) as i64);
        let silent_since_due = self.owner_seen.is_none_or(|seen| seen <= due);
        if now - idle_since > STALE_AFTER_S || (overdue && silent_since_due) { Standing::Stale { idle_since } } else { Standing::Live }
    }
}
