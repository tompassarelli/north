use super::handoff_record::edited_paths;
use crate::hook_input::{HookInput, Verdict};
use std::path::Path;

pub fn decide(input: &HookInput) -> Verdict {
    let cwd = input.cwd().map(Path::new).map(Path::to_path_buf).or_else(|| std::env::current_dir().ok()).unwrap_or_default();
    match crate::guard::host::file_tool_refusal(&edited_paths(input), &cwd) {
        Some(path) => Verdict::Deny(format!("file-tool-reach: `{path}` is outside what this worker's shell may write. The Edit and Write tools run in Claude Code's own process, which the shell's sandbox does not confine, so this hook judges them by the same boundary: the session's lane, its git state, scratch and caches, its own spool and the guards' records. The fleet's session records, queue and archive, the client profiles and settings, north's other state and every main checkout are not writable. Work in your lane, and use `north fleet` to act on a session.")),
        None => Verdict::Allow,
    }
}
