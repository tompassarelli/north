//! The private-text judge for a confined caller (north#1553): a worker's domain reads no model account's login (readable.rs), so `north check privacy judge` inside it has no model to ask. The request is the words `privacy-judge` or `privacy-judge at-once` and the text on the caller's standard input; the broker runs the judge outside every domain with its own environment and relays only the judge's answer, every token withheld. It is served only to a fleet session outside every lane-only scope.

use north_base::platform::private_text::{ASKING_STATUS, AT_ONCE_ENV, JUDGE_ARGS};

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

/// Whether the judge's exit `code` is an answer the caller reads on its own: clean, findings, or another judge holding the turn.
pub(super) fn answered(code: u8) -> bool {
    code <= 1 || i32::from(code) == ASKING_STATUS
}
