use crate::hook::{HookInput, Verdict};

const MARKER: &str = "/agents/handoffs/";
const NAMES: [&str; 4] = ["decision", "audit", "review", "research"];
const SCRATCH: [&str; 2] = ["brief", "status"];

const ADVICE: &str = "handoff-record: handoff files are scratch and are not backed up. A decision, audit, review or research record belongs in the repository that owns it: public method in docs/rationale/ and docs/references.md, Tom-specific facts and full reviews in south agents/decisions/. Write it there and leave only a pointer in the handoff.";

pub fn evaluate(input: &HookInput) -> Verdict {
    if crate::guard::activation::authoring_guards_off("north-hook") {
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

