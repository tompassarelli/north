#![cfg(test)]

use super::native_evidence::decide_with;
use crate::hook::{HookInput, Verdict};
use serde_json::json;

fn verdict(command: &str) -> Verdict {
    let payload = json!({"tool_name": "Bash", "tool_input": {"command": command}});
    decide_with(&HookInput::parse(payload.to_string().as_bytes()).unwrap(), |_, _| Ok(()))
}

// KEEP(bug): 1 of 3 sampled native-evidence refusals was false: `wisp.ts pad` with no pad named only prints the pad usage; north#706
#[test]
fn wisp_pad_is_refused_only_when_it_names_a_pad() {
    for command in ["bun scripts/wisp.ts pad 2>&1 | head -20", "bun wisp pad", "bun wisp pad --help"] {
        assert!(matches!(verdict(command), Verdict::Allow), "{command}");
    }
    for command in ["bun scripts/wisp.ts pad rising-whirlwind.pad", "bun wisp pad run.pad --seed 3"] {
        assert!(matches!(verdict(command), Verdict::Deny(_)), "{command}");
    }
}
