use super::{HookReply, hook_json, hook_reply, store};
use crate::foundation::hook_deadline::{errors_file, record_error, within};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const PAYLOAD_LIMIT: u64 = 1048576;
const HOOK_ID: &str = "prompt-grounding";
const DEADLINE: Duration = Duration::from_millis(1000);
const RETRIEVAL: Duration = Duration::from_millis(600);
const LATE_NOTE: &str = "Grounding stopped at its 1 s deadline because the index was slow, so this message was not grounded; run `north grounding query --query TEXT` if it matters.";

pub struct Gate<'a> {
    pub guards_off: &'a dyn Fn() -> bool,
    pub errors: PathBuf,
}

pub fn run(guards_off: fn() -> bool, grounded: fn(&str) -> bool) {
    let started = Instant::now();
    let mut bytes = Vec::new();
    if std::io::stdin().take(PAYLOAD_LIMIT + 1).read_to_end(&mut bytes).is_err() {
        return;
    }
    let errors = errors_file();
    let late_errors = errors.clone();
    let work = move || {
        let gate = Gate { guards_off: &guards_off, errors };
        decide(&bytes, &gate, |raw| hook_reply(raw, &grounded, Some(started + RETRIEVAL)))
    };
    let finished = within(started + DEADLINE, work);
    if finished.is_none() {
        store::kill_live();
    }
    let output = conclude(finished, &late_errors);
    if let Some(output) = output {
        let _ = writeln!(std::io::stdout().lock(), "{output}");
    }
    let _ = std::io::stdout().flush();
    std::process::exit(0);
}

fn conclude(finished: Option<Option<String>>, errors: &Path) -> Option<String> {
    finished.unwrap_or_else(|| {
        hook_error(errors, "deadline");
        Some(hook_json(LATE_NOTE).to_string())
    })
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
    record_error(file, HOOK_ID, kind);
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stuck_decider_is_abandoned_at_the_deadline_with_a_note_instead_of_hanging() {
        let started = Instant::now();
        let stuck = within(started + Duration::from_millis(100), || {
            std::thread::sleep(Duration::from_secs(5));
            Some("never".to_owned())
        });
        assert!(stuck.is_none());
        assert!(started.elapsed() < Duration::from_secs(2));
        let dir = tempfile::tempdir().unwrap();
        let errors = dir.path().join("errors.tsv");
        let note = conclude(stuck, &errors).unwrap();
        assert!(note.contains(LATE_NOTE) && note.contains("additionalContext"));
        assert!(std::fs::read_to_string(&errors).unwrap().ends_with("\tprompt-grounding\tdeadline\n"));
    }
}
