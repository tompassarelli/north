use crate::hook_input::{HookInput, Verdict};

const ENDED: &str = "the finish call queued the lane, so the turn ends here; a lander refusal starts the next one";

/// Ends a session's turn at its accepted `finish` call, so the client asks the model nothing after the ticket.
/// Claude Code fires PostToolUse only for a call whose result is no error, so a refused finish reaches the model through PostToolUseFailure.
pub fn evaluate(input: &HookInput) -> Verdict {
    if input.data["hook_event_name"] == "PostToolUse" && input.tool_name() == crate::fleet::tickets::FINISH_TOOL { Verdict::End(ENDED.to_owned()) } else { Verdict::Allow }
}
