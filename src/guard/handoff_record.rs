use crate::hook::{HookInput, Verdict};

const MARKER: &str = "/agents/handoffs/";
const NAMES: [&str; 4] = ["decision", "audit", "review", "research"];
const SCRATCH: [&str; 2] = ["brief", "status"];

const ADVICE: &str = "handoff-record: handoff files are scratch and are not backed up. A decision, audit, review or research record belongs in the repository that owns it: public method in docs/rationale/ and docs/references.md, Tom-specific facts and full reviews in south agents/decisions/. Write it there and leave only a pointer in the handoff.";

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::hook::authoring_guards_off("north-hook") {
        return Verdict::Allow;
    }
    decide(input)
}

pub(super) fn decide(input: &HookInput) -> Verdict {
    if edited_paths(input).iter().any(|path| durable_record(path)) {
        Verdict::Advise(ADVICE.into())
    } else {
        Verdict::Allow
    }
}

pub(super) fn edited_paths(input: &HookInput) -> Vec<String> {
    let tool_input = &input.tool_input;
    let direct = ["file_path", "filePath", "notebook_path"]
        .into_iter()
        .filter_map(|key| tool_input[key].as_str())
        .map(str::to_owned);
    let patch = ["command", "input", "patch"]
        .into_iter()
        .filter_map(|key| tool_input[key].as_str())
        .chain(tool_input.as_str())
        .flat_map(str::lines)
        .filter_map(|line| {
            let line = line.strip_prefix("*** ")?;
            let (kind, path) = line.split_once(':')?;
            matches!(kind, "Add File" | "Update File" | "Move to").then(|| path.trim().to_owned())
        });
    direct.chain(patch).collect()
}

fn durable_record(path: &str) -> bool {
    let Some((_, tail)) = path.split_once(MARKER) else {
        return false;
    };
    let name = tail.rsplit('/').next().unwrap_or("").to_lowercase();
    !name.ends_with(".log")
        && NAMES.iter().any(|word| name.contains(word))
        && !SCRATCH.iter().any(|word| name.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(tool: &str, input: serde_json::Value) -> HookInput {
        let payload = json!({"tool_name": tool, "cwd": "/tmp", "tool_input": input});
        HookInput::parse(&serde_json::to_vec(&payload).unwrap()).unwrap()
    }

    const HANDOFFS: &str = "/home/user/.local/state/agents/handoffs";

    #[test]
    fn durable_records_under_handoffs_advise_and_everything_else_passes() {
        for name in ["north-rebuild/pstack-review-opus.md", "x-decision.md", "AUDIT.md", "a/research-notes.txt"] {
            let path = format!("{HANDOFFS}/{name}");
            for input in [
                call("Write", json!({"file_path": path})),
                call("Edit", json!({"file_path": path})),
                call("MultiEdit", json!({"file_path": path})),
                call("apply_patch", json!({"command": format!("*** Begin Patch\n*** Add File: {path}\n+x\n*** End Patch")})),
                call("functions.apply_patch", json!({"input": format!("*** Update File: {path}")})),
            ] {
                assert_eq!(decide(&input), Verdict::Advise(ADVICE.into()), "{} {path}", input.tool_name());
            }
        }
        for path in [
            format!("{HANDOFFS}/pstack-review-brief.md"),
            format!("{HANDOFFS}/audit-status.md"),
            format!("{HANDOFFS}/audit-run.log"),
            format!("{HANDOFFS}/worker-notes.md"),
            "/home/user/code/south/main/agents/decisions/harness/audit.md".to_owned(),
            "/home/user/code/north/main/docs/rationale/pstack.md".to_owned(),
            "/tmp/handoffs/review.md".to_owned(),
        ] {
            assert_eq!(decide(&call("Write", json!({"file_path": path}))), Verdict::Allow, "{path}");
        }
        assert_eq!(decide(&call("Bash", json!({"command": "ls"}))), Verdict::Allow);
        let path = format!("{HANDOFFS}/north-rebuild/pstack-decision.md");
        let output = crate::hook::evaluate(&call("Write", json!({"file_path": path})), &[decide], "PreToolUse").unwrap();
        let output: serde_json::Value = serde_json::from_str(&crate::platform::pyjson::dumps(&output)).unwrap();
        assert_eq!(output, json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "additionalContext": ADVICE}}));
    }
}
