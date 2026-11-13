#![cfg(test)]

use super::native_evidence::decide;
use crate::hook_input::{Evidence, HookInput, Verdict};
use serde_json::json;

fn verdict(command: &str) -> Verdict {
    let payload = json!({"tool_name": "Bash", "tool_input": {"command": command}});
    decide(&HookInput::parse(payload.to_string().as_bytes()).unwrap())
}

#[test]
fn wisp_pad_is_refused_only_when_it_names_a_pad_and_a_solo_run_hands_up_its_row() {
    for command in ["bun scripts/wisp.ts pad 2>&1 | head -20", "bun wisp pad", "bun wisp pad --help"] {
        assert!(matches!(verdict(command), Verdict::Allow), "{command}");
    }
    for command in ["bun scripts/wisp.ts pad rising-whirlwind.pad", "bun wisp pad run.pad --seed 3"] {
        assert!(matches!(verdict(command), Verdict::Deny(_)), "{command}");
    }
    let solo = "NORTH_NATIVE_SOLO='timing the pad' bun wisp pad run.pad";
    assert!(matches!(verdict(solo), Verdict::Record(Evidence::NativeSolo { reason, command, .. }) if reason == "timing the pad" && command == solo));
}
