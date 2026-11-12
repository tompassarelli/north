//! The owner's hold on every API organization (`[accounts.claude] api_hold` in the orchestration policy,
//! docs/rationale/spend-rule.md): while the policy records it, or while the policy cannot be read, no caller of any
//! role draws an `api` account, operational callers included. Nothing in north writes or lifts it; only an edit of that record does.
use std::path::Path;

use toml_edit::{DocumentMut, Item};

/// The hold that stands: why every API organization is held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hold {
    pub why: String,
}

impl Hold {
    /// The one line that refuses `account` while the hold stands.
    pub fn line(&self, account: &str) -> String {
        format!("{account} is held: {}", self.why)
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
    let why = format!("every API organization is held since {} ({}); no caller of any role draws it, and only the policy's api_hold record lifts it", field("since", "an unrecorded day"), field("reason", "no reason recorded"));
    Some(Hold { why })
}

/// The policy at `policy`, or the hold that stands because it cannot be read or parsed.
pub fn read(policy: &Path) -> Result<DocumentMut, Hold> {
    let text = std::fs::read_to_string(policy).map_err(|error| error.to_string());
    text.and_then(|text| text.parse::<DocumentMut>().map_err(|error| error.to_string()))
        .map_err(|error| Hold { why: format!("the policy at {} cannot be read ({}), so every API organization counts as held until it reads again", policy.display(), error.trim()) })
}

/// The hold that stands under the policy file at `policy`: the one it records, or one because it cannot be read.
pub fn at(policy: &Path) -> Option<Hold> {
    read(policy).map_or_else(Some, |cfg| declared(&cfg))
}

/// Whether `account` names an API organization: the pooled `api` account or `api:ORG`, with or without its `claude:` provider.
pub fn api(account: &str) -> bool {
    let label = account.strip_prefix("claude:").unwrap_or(account);
    label == "api" || label.starts_with("api:")
}
