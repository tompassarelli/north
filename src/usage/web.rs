//! Reset availability read from claude.ai's usage page and cached by the browser reader.
use std::fs;
use std::path::Path;

use serde_json::Value;

use super::py;
use crate::foundation::clock::Timestamp;

const KINDS: [&str; 2] = ["full", "five_hour"];

pub(super) struct Resets {
    pub source: String,
    pub grants: Vec<Grant>,
}

pub(super) struct Grant {
    pub name: String,
    pub amount: f64,
    pub expires: Option<String>,
}

pub(super) fn resets(state: &Path, account: Option<&str>, now: Timestamp) -> Result<Option<Resets>, String> {
    let Some(cache) = fs::read_to_string(state.join("agents/usage/claude-web.json")).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok()) else {
        return Ok(None);
    };
    let entry = cache["accounts"].as_object().and_then(|entries| entries.values().find(|entry| account.is_some() && entry["account_label"].as_str() == account));
    let Some(entry) = entry else { return Ok(None) };
    let valid_until = entry["valid_until"].as_str().filter(|text| !text.is_empty());
    if let Some(until) = valid_until
        && py::parse(until)? <= now
    {
        return Ok(None);
    }
    let counted = |kind: &str| entry["resets"][kind]["count"].as_i64().or_else(|| entry["resets"][kind]["count"].as_u64().and_then(|n| i64::try_from(n).ok()));
    if KINDS.iter().any(|kind| counted(kind).is_none()) {
        return Ok(None);
    }
    let mut source = entry["source"].as_str().filter(|text| !text.is_empty()).unwrap_or("claude.ai settings/usage via Chrome").to_owned();
    match valid_until {
        Some(until) => {
            let at = py::civil(py::parse(until)?);
            source.push_str(&format!("; until {} {} {:02}:{:02}Z", at.day(), py::month(&at), at.hour(), at.minute()));
        }
        None => {
            let at = py::civil(py::parse(entry["read_at"].as_str().unwrap_or_default())?);
            source.push_str(&format!(", read {:02}:{:02}Z", at.hour(), at.minute()));
        }
    }
    let grants = entry["resets"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(kind, _)| KINDS.contains(&kind.as_str()))
        .map(|(kind, grant)| Grant {
            name: format!("cedar_ember/{kind}"),
            amount: counted(kind).unwrap_or_default() as f64,
            expires: grant["expires"].as_str().filter(|text| !text.is_empty()).map(str::to_owned),
        })
        .collect();
    Ok(Some(Resets { source, grants }))
}
