use crate::guard::proxy_role::{self, World};
use crate::hook_input::{HookInput, Verdict};

const UNIT: &str = "question-box";
pub const QUESTION_TOOL: &str = "AskUserQuestion";

pub fn evaluate(input: &HookInput) -> Verdict {
    decide(&proxy_role::Live, input)
}

pub fn decide(world: &impl World, input: &HookInput) -> Verdict {
    if input.tool_name() != QUESTION_TOOL || !proxy_role::is_proxy(world, input) {
        return Verdict::Allow;
    }
    Verdict::Deny(format!(
        "BLOCKED ({UNIT}): a question box stops the proxy's whole session until Tom answers, and a decision for Tom never stops work. Open a gate instead: `north gate open --session NAME --question TEXT --option TEXT --option TEXT --recommend N --default N --default-after 8h` (leave out --default and --default-after only for an ask-once class, which makes it a for-Tom gate that holds just its own box), keep working, and put the choice last in your report as a multiple-choice question in text, numbering its options exactly as the gate does (the first `--option` is 1, the second 2), because Tom types back a bare number and the relay matches it against the gate's numbering."
    ))
}
