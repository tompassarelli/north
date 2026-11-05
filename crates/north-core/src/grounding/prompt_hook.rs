use super::{HookReply, hook_json, hook_reply};
use crate::foundation::clock::Timestamp;
use std::{
    env,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const PAYLOAD_LIMIT: u64 = 1048576;
const HOOK_ID: &str = "prompt-grounding";

pub struct Gate<'a> {
    pub guards_off: &'a dyn Fn() -> bool,
    pub errors: PathBuf,
}

pub fn run(guards_off: &dyn Fn() -> bool, grounded: &dyn Fn(&str) -> bool) {
    let mut bytes = Vec::new();
    if std::io::stdin().take(PAYLOAD_LIMIT + 1).read_to_end(&mut bytes).is_err() {
        return;
    }
    let home = PathBuf::from(env::var_os("HOME").unwrap_or_default());
    let errors = env::var_os("AGENT_HOOK_ERRORS").map_or_else(|| home.join(".local/state/agents/hooks/errors.tsv"), PathBuf::from);
    let gate = Gate { guards_off, errors };
    if let Some(output) = decide(&bytes, &gate, |raw| hook_reply(raw, grounded)) {
        let _ = writeln!(std::io::stdout().lock(), "{output}");
    }
}

pub(super) fn decide(bytes: &[u8], gate: &Gate, reply: impl FnOnce(&str) -> super::Result<HookReply>) -> Option<String> {
    if bytes.len() as u64 > PAYLOAD_LIMIT {
        return None;
    }
    let raw = String::from_utf8_lossy(bytes);
    if raw.contains("\"agent_id\"") || raw.contains("\"agent_type\"") || (gate.guards_off)() {
        return None;
    }
    match reply(&raw) {
        Ok(HookReply::Context(text)) => Some(hook_json(&text).to_string()),
        Ok(HookReply::Silent) => None,
        Ok(HookReply::Unparsable) => {
            hook_error(&gate.errors, "unparsable-payload");
            None
        }
        Err(_) => {
            hook_error(&gate.errors, "decider-exception");
            None
        }
    }
}

fn hook_error(file: &Path, kind: &str) {
    if let Some(parent) = file.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(out, "{}\t{HOOK_ID}\t{kind}", Timestamp::now().iso());
    }
}

#[cfg(test)]
#[path = "prompt_hook_tests.rs"]
mod tests;
