use std::fs;
use std::path::Path;

use crate::fleet::records::read_record;
use crate::hook::{HookInput, Verdict};

pub const MESSAGE_TOOL: &str = "SendMessage";

pub fn evaluate(input: &HookInput) -> Verdict {
    let root = crate::fleet::registry::root_here().ok();
    notice(input, root.as_deref()).map_or(Verdict::Allow, Verdict::Advise)
}

pub fn notice(input: &HookInput, root: Option<&Path>) -> Option<String> {
    if input.tool_name() != MESSAGE_TOOL {
        return None;
    }
    let target = input.tool_input.get("to")?.as_str()?;
    let name = target.split(" [").next().unwrap_or(target).trim();
    if name.is_empty() || root.is_some_and(|root| is_fleet_session(root, name)) {
        return None;
    }
    Some(format!(
        "subagent-message-delay: {name} is not a live fleet session, so if it is an in-process Agent subagent it reads \
         this message only at its next tool round. A subagent inside a long tool call (a build, test, sleep or wait) \
         sees it when that call returns, not before. A message that must land now goes to a fleet session, which \
         takes native delivery within seconds."
    ))
}

fn is_fleet_session(root: &Path, name: &str) -> bool {
    fs::read_dir(root).into_iter().flatten().flatten().filter_map(|entry| read_record(&entry.path()).ok()).any(|record| record.name == name)
}
