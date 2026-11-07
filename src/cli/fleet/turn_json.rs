use serde_json::{Value, json};

use crate::fleet::turns::Turn;

pub(super) fn turn_json(turn: &Turn) -> Value {
    json!({
        "exit": turn.exit.label(),
        "ended_ms": turn.ended_ms,
        "turns": turn.turns,
        "tokens_in": turn.tokens.input,
        "tokens_out": turn.tokens.output,
        "tokens_cache_read": turn.tokens.cache_read,
        "tokens_cache_write": turn.tokens.cache_write,
    })
}
