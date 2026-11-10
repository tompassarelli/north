//! The claude_only rule in front of `north fleet launch` (north#1039): a DeepSeek worker on an issue or box whose subject
//! stays on Claude is refused, whether dispatch or a person launches it.
use serde_json::Value;

use crate::dispatch::{IssueRef, OWNER};
use crate::error::{NorthError, NorthResult};
use crate::fleet::claude_only::{ClaudeOnly, DEEPSEEK_PROVIDERS};
use crate::fleet::model::LaunchRequest;
use crate::fleet::runtime::now_ms;
use crate::github_mirror::store::{Mirror, OPEN_ISSUES, key};

/// Refuses a DeepSeek launch whose task's title or claimed box matches a claude_only pattern, and one whose task title cannot be read.
pub(super) fn admit(request: &LaunchRequest) -> NorthResult<()> {
    let provider = request.provider.label();
    if !DEEPSEEK_PROVIDERS.contains(&provider) {
        return Ok(());
    }
    let only = ClaudeOnly::read();
    let box_text = request.claimed_box();
    for task in request.tasks() {
        let reference = IssueRef::parse(&task).ok_or_else(|| NorthError::Refused(format!("{task} names no issue, so its title cannot be checked against claude_only before a {provider} launch")))?;
        let title = title(&reference).map_err(|why| NorthError::Refused(format!("cannot read the title of {reference} to check it against claude_only before a {provider} launch: {why}")))?;
        if let Some(refusal) = only.refusal(provider, &reference.to_string(), &title, box_text.as_deref()) {
            return Err(NorthError::Refused(refusal));
        }
    }
    Ok(())
}

fn title(reference: &IssueRef) -> Result<String, String> {
    let mirrored = Mirror::local().fresh(&key(&reference.repo, OPEN_ISSUES), now_ms() as f64 / 1000.0);
    let node = mirrored.as_ref().and_then(Value::as_array).and_then(|nodes| nodes.iter().find(|node| node["number"].as_u64() == Some(reference.number)).cloned());
    let node = match node {
        Some(node) => node,
        None => crate::platform::github::json(&["issue", "view", &reference.number.to_string(), "-R", &format!("{OWNER}/{}", reference.repo), "--json", "title"]).map_err(|why| why.trim().to_owned())?,
    };
    node["title"].as_str().map(str::to_owned).ok_or_else(|| "the issue has no title".to_owned())
}
