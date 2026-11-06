use crate::guard::proxy_role;
use crate::hook::{HookInput, Verdict};
use std::process::{Command, Stdio};

pub fn evaluate(input: &HookInput) -> Verdict {
    if proxy_role::is_tom_window(&proxy_role::Live, input) {
        let _ = Command::new("north").args(["grounding", "distill", "capture"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    }
    Verdict::Allow
}
