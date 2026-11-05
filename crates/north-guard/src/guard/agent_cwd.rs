use crate::hook::{HookInput, Verdict};

const READ_ONLY: [&str; 3] = ["Explore", "Plan", "claude-code-guide"];

fn lane_of(cwd: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = cwd.split('/').collect();
    let at = parts.iter().position(|part| *part == "worktrees")?;
    let (repo, lane) = (parts.get(at.checked_sub(1)?)?, parts.get(at + 1)?);
    if repo.is_empty() || lane.is_empty() {
        return None;
    }
    let root = parts[..at + 2].join("/");
    Some((root, format!("{repo}/worktrees/{lane}")))
}

pub fn evaluate(input: &HookInput) -> Verdict {
    if input.data["tool_name"] != "Agent" {
        return Verdict::Allow;
    }
    if input.tool_input["subagent_type"].as_str().is_some_and(|kind| READ_ONLY.contains(&kind)) {
        return Verdict::Allow;
    }
    let Some(cwd) = input.data["cwd"].as_str() else {
        return Verdict::Allow;
    };
    let Some((root, tail)) = lane_of(cwd) else {
        return Verdict::Allow;
    };
    let prompt = input.tool_input["prompt"].as_str().unwrap_or("");
    if prompt.contains(&tail) {
        return Verdict::Allow;
    }
    Verdict::Deny(format!(
        "This session's cwd is the lane worktree {root}, and an in-process worker inherits it: it would commit and switch branches inside this lane while you land from it. cd to the main checkout first (~/code/<repo>/main) and spawn from there, or name the lane path {tail} in the brief if the worker is meant to work in it."
    ))
}
