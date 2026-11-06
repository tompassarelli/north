//! `north strategy`: the decision log's check and retention, and the replay of its cases.

pub fn run(arguments: &[String]) -> u8 {
    match arguments.split_first() {
        Some((verb, rest)) if verb == "replay" => north_replay::run(rest),
        _ => crate::ops::strategy_log::run(arguments),
    }
}
