use super::receipts::is_final;

pub fn ended_on_a_wait(reply: &str) -> bool {
    let reply = reply.trim();
    !reply.is_empty() && !is_final(reply) && !reply.ends_with('?')
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
}
