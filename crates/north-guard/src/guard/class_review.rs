//! The coordinator's stop hook over its hourly class review (north#1577): its turn does not end while a harness-loss class
//! seen twice in the last hour has no owner issue.

use north_unblocks::review::{HOUR_SECONDS, Owners, classes, mirror_closed, refusal, since, threads_db};

use crate::hook_input::{HookInput, Verdict};

pub fn evaluate(input: &HookInput) -> Verdict {
    if !super::coordinator_role::is_coordinator(&super::coordinator_role::Live, input) {
        return Verdict::Allow;
    }
    let now = crate::foundation::clock::Timestamp::now().unix_seconds();
    let judged = since(&threads_db(), now - HOUR_SECONDS).and_then(|reading| classes(&reading, &Owners::here(), &mirror_closed));
    match judged {
        Ok(found) => refusal(&found).map_or(Verdict::Allow, Verdict::Deny),
        Err(why) => Verdict::Advise(format!("class-review: the harness losses of the last hour are unread: {why}")),
    }
}
