use std::fs;
use std::path::Path;

use crate::fleet::model::Role;
use crate::fleet::records::{past_records, read_record};
use crate::fleet::runtime::SESSION_ENV;
use crate::hook::{HookInput, Verdict};

pub const TIMER_TOOLS: [&str; 2] = ["ScheduleWakeup", "CronCreate"];

pub fn evaluate(input: &HookInput) -> Verdict {
    let session = std::env::var(SESSION_ENV).ok().filter(|session| !session.is_empty());
    let Some(root) = session.as_ref().and_then(|_| crate::fleet::registry::root_here().ok()) else {
        return Verdict::Allow;
    };
    match refusal(input, session.as_deref(), &root) {
        Some(reason) if !crate::guard::activation::authoring_guards_off("north-hook") => Verdict::Deny(reason),
        _ => Verdict::Allow,
    }
}

pub fn refusal(input: &HookInput, session: Option<&str>, root: &Path) -> Option<String> {
    let tool = input.tool_name();
    if !TIMER_TOOLS.contains(&tool) || input.data["agent_type"].as_str().is_some() {
        return None;
    }
    let session = session.filter(|session| !session.is_empty())?;
    is_lead(session, root).then(|| {
        format!(
            "lead-timer-guard: {session} is a Fleet lead, and a lead does not wake on timers, so {tool} is refused. \
             End your turn instead: the fleet wakes you on child reports, registered waits (`north fleet wait`) and \
             `north flow` alerts, each delivered as a message."
        )
    })
}

pub fn is_lead(session: &str, root: &Path) -> bool {
    match read_record(&root.join(session)).ok().and_then(|record| record.role) {
        Some(role) => role != Role::Worker,
        None => session.split('-').any(|part| part == "lead") || has_children(session, root),
    }
}

fn has_children(session: &str, root: &Path) -> bool {
    let current = fs::read_dir(root).into_iter().flatten().flatten().filter_map(|entry| read_record(&entry.path()).ok());
    current.chain(past_records(root)).any(|record| record.parent.as_deref() == Some(session))
}
