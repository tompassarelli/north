use std::time::Duration;

use super::receipts::is_final;

pub const WAKE_MARK: &str = "[north: your turn ended on a wait";
pub const WAKE_AFTER: [Duration; 3] = [Duration::from_secs(5 * 60), Duration::from_secs(15 * 60), Duration::from_secs(45 * 60)];

const QUOTE_CHARS: usize = 240;

pub fn ended_on_a_wait(reply: &str) -> bool {
    let reply = reply.trim();
    !reply.is_empty() && !is_final(reply) && !reply.ends_with('?')
}

pub fn is_wake(prompt: &str) -> bool {
    prompt.trim_start().starts_with(WAKE_MARK)
}

pub fn wake(reply: &str, attempt: usize) -> String {
    let quoted: String = reply.trim().chars().take(QUOTE_CHARS).collect();
    format!(
        "{WAKE_MARK}, wake {} of {}] You stopped with \"{quoted}\" and no Done, Not done, Split, Stop or Blocked report. If the run you waited on has finished, read its result and continue to a final report. If it is still running, arm `job-run --notify` or Monitor on it and end your turn only after that, so its exit wakes you.",
        attempt + 1,
        WAKE_AFTER.len(),
    )
}

pub fn stalled(child: &str, reply: &str) -> String {
    let quoted: String = reply.trim().chars().take(QUOTE_CHARS).collect();
    format!("[from {child} · ended on a wait · woken {} times without a final report] Last reply: \"{quoted}\". Read it with `north fleet show {child}`.", WAKE_AFTER.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_prose_stop_with_no_final_opening_and_no_question_is_a_wait() {
        assert!(ended_on_a_wait("Still building on nexus."));
        assert!(ended_on_a_wait("Waiting on the land check; I will resume when it ends."));
        assert!(!ended_on_a_wait("Done: landed abc123."));
        assert!(!ended_on_a_wait("**Not done:** the run failed"));
        assert!(!ended_on_a_wait("Which target do you want?"));
        assert!(!ended_on_a_wait("  \n"));
    }

    #[test]
    fn a_wake_is_recognised_by_its_mark_and_quotes_the_stopping_reply() {
        let text = wake("Still building on nexus.", 1);
        assert!(is_wake(&text));
        assert!(text.contains("wake 2 of 3") && text.contains("Still building on nexus."), "{text}");
        assert!(!is_wake("job-run: nexus-build exited 0; output in /tmp/x.log"));
    }
}
