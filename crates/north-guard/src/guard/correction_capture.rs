use crate::guard::coordinator_role;
use crate::hook_input::{HookInput, Verdict};
use std::process::{Command, Stdio};

pub fn evaluate(input: &HookInput) -> Verdict {
    if coordinator_role::is_tom_window(&coordinator_role::Live, input) {
        let _ = Command::new("north").args(["graph", "ground", "distill", "capture"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    }
    Verdict::Allow
}
