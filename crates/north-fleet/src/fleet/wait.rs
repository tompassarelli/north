use super::receipts::is_final;

pub fn ended_on_a_wait(reply: &str) -> bool {
    let reply = reply.trim();
    !reply.is_empty() && !is_final(reply) && !reply.ends_with('?')
}

