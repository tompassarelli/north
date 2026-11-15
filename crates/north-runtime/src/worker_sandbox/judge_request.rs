//! The private-text judge for a confined caller (north#1553): a worker's domain reads no model account's login (readable.rs), so `north check privacy judge` inside it has no model to ask. The request is the words `privacy-judge` or `privacy-judge at-once` and the text on the caller's standard input; the broker runs the judge outside every domain with its own environment and relays only the judge's answer, every token withheld. It is served only to a fleet session outside every lane-only scope.

use std::io::Write;
use std::path::Path;

use north_base::platform::private_text::{ASKING_STATUS, AT_ONCE_ENV, CLAUDE_ENV, JUDGE_ARGS};

pub(super) const VERB: &str = "privacy-judge";
pub(super) const AT_ONCE: &str = "at-once";

/// Whether `arguments` name this request, and whether the caller judges at once.
pub(super) fn asked(arguments: &[String]) -> Option<bool> {
    match arguments {
        [verb] if verb == VERB => Some(false),
        [verb, flag] if verb == VERB && flag == AT_ONCE => Some(true),
        _ => None,
    }
}

/// The judge the broker starts for a request that judges `at_once` or not: its words and the variables added to the broker's own.
pub(super) fn judge(at_once: bool) -> (Vec<String>, Vec<(String, String)>) {
    (JUDGE_ARGS.map(str::to_owned).to_vec(), at_once.then(|| (AT_ONCE_ENV.to_owned(), "1".to_owned())).into_iter().collect())
}

/// `environment` with the two programs the judge starts by name given as absolute paths, its `PATH` untouched: a broker the user manager starts on a build host has a `PATH` holding neither, so every ask failed with no such file. `north` is the broker's own build and `claude` the first one on the broker's `PATH`, else `home`'s `.local/bin/claude`; a path that does not exist is written to `refusals`, naming it, and starts nothing.
pub(super) fn programs(mut environment: Vec<(String, String)>, north: &Path, home: &Path, refusals: &mut impl Write) -> Option<Vec<(String, String)>> {
    let path = environment.iter().find(|(name, _)| name == "PATH").map(|(_, value)| value.clone()).unwrap_or_default();
    let claude = std::env::split_paths(&path).map(|entry| entry.join("claude")).chain([home.join(".local/bin/claude")]).find(|candidate| candidate.is_absolute() && candidate.is_file()).unwrap_or_else(|| home.join(".local/bin/claude"));
    for program in [north, claude.as_path()] {
        if !program.is_file() {
            let _ = writeln!(refusals, "privacy judge: the private-text judge needs {}, which does not exist", program.display());
            return None;
        }
    }
    environment.retain(|(name, _)| name != "NORTH_BIN" && name != CLAUDE_ENV);
    environment.extend([("NORTH_BIN".to_owned(), north.to_string_lossy().into_owned()), (CLAUDE_ENV.to_owned(), claude.to_string_lossy().into_owned())]);
    Some(environment)
}

/// Whether the judge's exit `code` is an answer the caller reads on its own: clean, findings, or another judge holding the turn.
pub(super) fn answered(code: u8) -> bool {
    code <= 1 || i32::from(code) == ASKING_STATUS
}
