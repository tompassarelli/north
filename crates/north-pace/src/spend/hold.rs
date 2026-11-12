//! The owner's hold on every API organization (`[accounts.claude] api_hold` in the orchestration policy,
//! docs/rationale/spend-rule.md): while the policy records it, no caller of any role draws an `api` account,
//! operational callers included. Nothing in north writes or lifts it; only an edit of that record does.
use std::path::Path;

use toml_edit::{DocumentMut, Item};

/// The recorded hold: the day it was set and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hold {
    pub since: String,
    pub reason: String,
}

impl Hold {
    /// The one line that refuses `account` while the hold stands.
    pub fn line(&self, account: &str) -> String {
        format!("{account} is held: every API organization is held since {} ({}); no caller of any role draws it, and only the policy's api_hold record lifts it", self.since, self.reason)
    }

    /// The refusal of `account` under the hold, or none for an account that is no API organization.
    pub fn refusal(&self, account: &str) -> Option<String> {
        api(account).then(|| self.line(account))
    }
}

/// The hold `[accounts.claude] api_hold` records in `cfg`, or none.
pub fn declared(cfg: &DocumentMut) -> Option<Hold> {
    let held = cfg.get("accounts")?.get("claude")?.get("api_hold")?;
    let field = |name: &str, missing: &str| held.get(name).and_then(Item::as_str).unwrap_or(missing).to_owned();
    Some(Hold { since: field("since", "an unrecorded day"), reason: field("reason", "no reason recorded") })
}

/// The hold the policy file at `policy` records, or none.
pub fn at(policy: &Path) -> Option<Hold> {
    std::fs::read_to_string(policy).ok().and_then(|text| text.parse::<DocumentMut>().ok()).as_ref().and_then(declared)
}

/// Whether `account` names an API organization: the pooled `api` account or `api:ORG`, with or without its `claude:` provider.
pub fn api(account: &str) -> bool {
    let label = account.strip_prefix("claude:").unwrap_or(account);
    label == "api" || label.starts_with("api:")
}
