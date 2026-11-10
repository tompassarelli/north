//! The coordinator's usage reset: notice a free grant, refuse the browser path the provider's
//! terms forbid, and record the account, time and Fable meter before and after. A future provider
//! command or API performs the reset here and records `performed` with the same fields.
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use serde_json::{Value, json};

use super::decide::SUBSCRIPTION_FLOOR;
use super::web;
use crate::foundation::clock::Timestamp;

/// The grants an account advertises on its usage page.
pub struct Offered {
    pub full: f64,
    pub five_hour: f64,
    pub expires: Option<String>,
    pub source: String,
}

/// What a check found: the meter is below the floor, no grant is offered, or a grant is offered
/// and the only path is the browser automation the Consumer Terms forbid.
#[derive(Debug, Eq, PartialEq)]
pub enum Outcome {
    BelowFloor,
    NoGrant,
    RefusedTerms,
}

impl Outcome {
    pub fn label(&self) -> &'static str {
        match self {
            Self::BelowFloor => "below-floor",
            Self::NoGrant => "no-grant",
            Self::RefusedTerms => "refused-terms",
        }
    }

    pub fn reason(&self) -> &'static str {
        match self {
            Self::BelowFloor => "the Fable meter is below the subscription floor",
            Self::NoGrant => "the account offers no usage reset",
            Self::RefusedTerms => {
                "Anthropic's Consumer Terms 3 forbid automated access to claude.ai and no provider reset command or API exists"
            }
        }
    }
}

/// The grants the cached usage page offers `account`, or `None` when the cache has no live reading
/// for it. The cache holds the page's own "reset for free" counts; a paid reset is not read.
pub fn offered(state: &Path, account: &str, now: Timestamp) -> Result<Option<Offered>, String> {
    let Some(found) = web::resets(state, Some(account), now)? else {
        return Ok(None);
    };
    let mut full = 0.0;
    let mut five_hour = 0.0;
    let mut expires = None;
    for grant in &found.grants {
        if grant.name.ends_with("/full") {
            full += grant.amount;
        } else {
            five_hour += grant.amount;
        }
        expires = expires.or_else(|| grant.expires.clone());
    }
    Ok(Some(Offered {
        full,
        five_hour,
        expires,
        source: found.source,
    }))
}

/// Whether the coordinator's account should spend a reset now.
pub fn assess(used: f64, offered: Option<&Offered>) -> Outcome {
    if used < SUBSCRIPTION_FLOOR.0 {
        return Outcome::BelowFloor;
    }
    let Some(offered) = offered else {
        return Outcome::NoGrant;
    };
    if offered.full + offered.five_hour <= 0.0 {
        return Outcome::NoGrant;
    }
    Outcome::RefusedTerms
}

/// Appends one check or attempt to `path`: the account, the time, the Fable meter before and after,
/// the grants offered and why the reset did or did not happen.
pub fn record(
    path: &Path,
    account: &str,
    at: Timestamp,
    meter_before: f64,
    meter_after: f64,
    offered: Option<&Offered>,
    outcome: &Outcome,
) -> Result<(), String> {
    let line = json!({
        "account": account,
        "at": at.iso(),
        "meter_before": meter_before,
        "meter_after": meter_after,
        "floor_pct": SUBSCRIPTION_FLOOR.0,
        "resets": offered.map_or(Value::Null, |offered| json!({
            "full": offered.full,
            "five_hour": offered.five_hour,
            "expires": offered.expires,
            "source": offered.source,
        })),
        "outcome": outcome.label(),
        "reason": outcome.reason(),
    });
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    writeln!(file, "{line}").map_err(|error| format!("{}: {error}", path.display()))
}
