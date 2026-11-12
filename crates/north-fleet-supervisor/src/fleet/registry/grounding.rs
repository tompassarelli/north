//! Experiment G2 of north#1147: until `UNTIL_MS`, every fresh worker launch on any host draws the
//! grounded or the plain arm in seeded equal shares, and its first prompt opens with a `Grounding:` line
//! naming the arm, so the prompt the ledger keeps for the run records it. What the grounded arm adds to the
//! prompt branches on the arm [`drawn`] returns.
use crate::router::settle;

const GROUNDED: &str = "grounded";
const PLAIN: &str = "plain";
const ARMS: [&str; 2] = [GROUNDED, PLAIN];
const FIELD: &str = "Grounding:";
const UNTIL_MS: u64 = 1_795_651_200_000;

/// The arm a launch named `name` at `now_ms` draws, or none once the experiment's period has ended.
pub fn drawn(name: &str, now_ms: u64) -> Option<&'static str> {
    (now_ms < UNTIL_MS).then(|| ARMS[settle::pick(&[1.0, 1.0], &format!("grounding {name}"))])
}

/// `prompt` opened by the line naming `arm`.
pub fn stamped(prompt: &str, arm: &str) -> String {
    format!("{FIELD} {arm}\n{prompt}")
}
