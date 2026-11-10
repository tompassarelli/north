//! When a NorthThreads claim still holds its issue: the one rule `threads claim`, `threads list`
//! and dispatch's survey share, so a worker is never refused by a claim dispatch saw as free.
use std::collections::BTreeSet;
use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use toml_edit::DocumentMut;

pub mod issues;
pub mod ledger;
pub mod lifecycle;
pub mod verbs;

pub static OVERDUE_RATIO: LazyLock<f64> = LazyLock::new(|| {
    let defaults: DocumentMut = north_base::platform::harness::table(north_base::platform::harness::DEFAULTS).parse().expect("harness/defaults.toml parses");
    defaults["overdue_ratio"]["value"].as_integer().map(|ratio| ratio as f64).or_else(|| defaults["overdue_ratio"]["value"].as_float()).expect("overdue_ratio is a number")
});

/// How long past its ETA, with nothing from its owner, a claim holds before anyone may take it.
pub const STALE_AFTER_S: i64 = 24 * 3600;

/// What joins an issue to the box one claim covers in the claims table's `item` column.
const BOX_MARK: char = '@';

/// The longest box slug a claim keeps.
const SLUG_LEN: usize = 48;

/// A box's text as a claim names it: lowercase words of letters and digits joined by `-`, so one box written with another checkbox, spacing or case is one claim; None when the text holds no word.
pub fn box_slug(text: &str) -> Option<String> {
    let trimmed = text.trim().trim_start_matches(['-', '*', ' ']).trim_start_matches("[ ]");
    let words: Vec<String> = trimmed.split(|c: char| !c.is_alphanumeric()).filter(|word| !word.is_empty()).map(str::to_lowercase).collect();
    let slug: String = words.join("-").chars().take(SLUG_LEN).collect();
    let slug = slug.trim_end_matches('-').to_owned();
    (!slug.is_empty()).then_some(slug)
}

/// Whether two box slugs name one box: equal, or one the leading whole words of the other, so a claim by `--box` with a box's first words and one by dispatch's slug of the whole box collide.
pub fn same_box(left: &str, right: &str) -> bool {
    let (short, long) = if left.len() <= right.len() { (left, right) } else { (right, left) };
    long.strip_prefix(short).is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
}

pub fn box_node(item: &str, text: &str) -> Option<String> {
    box_slug(text).map(|slug| format!("box:{item}:{slug}"))
}

/// The claims table's key for `item` held whole, or for the box `slug` of it.
pub fn key(item: &str, slug: Option<&str>) -> String {
    slug.map_or_else(|| item.to_owned(), |slug| format!("{item}{BOX_MARK}{slug}"))
}

/// The issue and the box slug a claims table key names.
pub fn split_key(key: &str) -> (String, Option<String>) {
    key.split_once(BOX_MARK).map_or_else(|| (key.to_owned(), None), |(item, slug)| (item.to_owned(), Some(slug.to_owned())))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Claim {
    pub item: String,
    /// The box of `item` this claim covers, or None when it covers the whole issue.
    pub slug: Option<String>,
    pub owner: String,
    pub since: i64,
    pub eta_min: Option<i64>,
    /// The latest event the owner recorded on the item.
    pub owner_seen: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Issue {
    Open { opened: i64 },
    Closed,
    Missing,
    /// GitHub or the native item table could not be read, so only the ETA rule applies.
    Unread,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Standing {
    Live,
    /// Past its ETA by [`STALE_AFTER_S`], or past [`OVERDUE_RATIO`] times its ETA, with no newer owner event since `idle_since`.
    Stale { idle_since: i64 },
    Void(&'static str),
}

/// A standing as `threads list --json` names it, so a reader of that listing matches a variant, not a string.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Label {
    Live,
    Stale,
    Void,
}

impl Standing {
    pub fn label(&self) -> Label {
        match self {
            Self::Live => Label::Live,
            Self::Stale { .. } => Label::Stale,
            Self::Void(_) => Label::Void,
        }
    }
}

impl Claim {
    /// The key this claim has in the claims table, and the instance its lifecycle record follows.
    pub fn key(&self) -> String {
        key(&self.item, self.slug.as_deref())
    }

    /// `alive` is the owners of the sessions alive now, or `None` when no fleet could be read, so only the issue and ETA rules apply.
    pub fn standing(&self, issue: Issue, alive: Option<&BTreeSet<String>>, now: i64) -> Standing {
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
