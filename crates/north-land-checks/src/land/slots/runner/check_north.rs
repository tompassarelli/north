use crate::land::slots::unit_reuse::quote;

/// The line a local check runs: `wrapped` when a wrapper rewrote `command`, else `command` with its leading `north` word naming `north`, the binary running the checks, so a north check line runs the north that judges the lane and never whichever one PATH finds first, such as a live link older than the line's verb.
pub(super) fn own(north: &str, command: &str, wrapped: String) -> String {
    if wrapped != command || north == "north" {
        return wrapped;
    }
    command.strip_prefix("north ").map_or(wrapped, |rest| format!("{} {rest}", quote(north)))
}
