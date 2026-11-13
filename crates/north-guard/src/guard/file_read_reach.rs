use crate::hook_input::{HookInput, Verdict};
use std::path::Path;

pub fn decide(input: &HookInput) -> Verdict {
    let cwd = input.cwd().map(Path::new).map(Path::to_path_buf).or_else(|| std::env::current_dir().ok()).unwrap_or_default();
    match crate::guard::host::file_read_refusal(&reached(input), &cwd) {
        Some(path) => Verdict::Deny(format!("file-read-reach: `{path}` is not readable from this worker.")),
        None => Verdict::Allow,
    }
}

/// The paths a reading tool call would read from: the file a Read or notebook read names, and the directory a Grep walks or a Glob's pattern starts from, since a search rooted at or above a path reads what lies beneath it.
pub(crate) fn reached(input: &HookInput) -> Vec<String> {
    let text = |key: &str| input.tool_input[key].as_str().filter(|text| !text.is_empty());
    let root = text("path").unwrap_or(".");
    match input.tool_name() {
        "Read" => text("file_path").map(str::to_owned).into_iter().collect(),
        "NotebookRead" => text("notebook_path").or_else(|| text("file_path")).map(str::to_owned).into_iter().collect(),
        "Grep" => vec![root.to_owned()],
        "Glob" => vec![Path::new(root).join(literal_prefix(text("pattern").unwrap_or_default())).display().to_string()],
        _ => Vec::new(),
    }
}

fn literal_prefix(pattern: &str) -> &str {
    let literal = pattern.find(['*', '?', '[', '{']).map_or(pattern.len(), |at| pattern[..at].rfind('/').map_or(0, |slash| slash + 1));
    &pattern[..literal]
}
