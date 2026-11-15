//! The coordinator's hand-carried patch (north#1577): a `git am` or `git apply` it runs, here or over ssh, is a manual unblock of
//! the class `hand-carried patch`, and the second within a day is refused unless the command names the open, owned issue that
//! fixes why a fix has to be carried by hand, as `NORTH_UNBLOCK_ISSUE=REPO#N`.

use std::sync::LazyLock;

use north_unblocks::{Admission, GitHub, Kind, Ledger};
use regex::Regex;

use crate::hook_input::{HookInput, Verdict};

pub const CLASS: &str = "a patch applied by hand outside a landing";
const ISSUE: &str = "NORTH_UNBLOCK_ISSUE=";

static APPLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?:^|[\s;&|(`'"])git(?:\s+-C\s+\S+)*\s+(?:am|apply)(?:[\s;&|)`'"]|$)([^;&|\n]*)"#).unwrap_or_else(|error| panic!("{error}")));

pub fn evaluate(input: &HookInput) -> Verdict {
    let command = input.command().filter(|_| input.is_shell()).unwrap_or_default();
    if !APPLY.is_match(command) || !super::coordinator_role::is_coordinator(&super::coordinator_role::Live, input) {
        return Verdict::Allow;
    }
    let now = u64::try_from(north_foundation::foundation::clock::Timestamp::now().unix_seconds()).unwrap_or_default();
    decide(command, &Admission { ledger: &Ledger::here(), issues: &GitHub, now })
}

/// Whether `command` applies a patch, and the ledger's verdict on it as the coordinator's unblock.
pub fn decide(command: &str, admission: &Admission) -> Verdict {
    let applies = APPLY.captures_iter(command).any(|found| !found.get(1).is_some_and(|rest| rest.as_str().split_whitespace().any(|word| matches!(word, "--check" | "--stat" | "--numstat" | "--summary"))));
    if !applies {
        return Verdict::Allow;
    }
    let issue = command.split_whitespace().find_map(|word| word.strip_prefix(ISSUE));
    match admission.admit(Kind::PatchCarry, CLASS, north_fleet_store::runtime::COORDINATOR, issue) {
        Ok(()) => Verdict::Allow,
        Err(why) => Verdict::Deny(format!("BLOCKED (repeat-unblock): {why}; here the issue goes in the command as `{ISSUE}REPO#N`")),
    }
}
