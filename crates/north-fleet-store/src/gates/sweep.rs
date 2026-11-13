use crate::error::{NorthError, NorthResult};

use super::record::{Closure, Decision, Gate, GateBook};

pub fn due(gate: &Gate, now_ms: u64) -> bool {
    gate.is_open() && gate.default_at_ms().is_some_and(|at| now_ms >= at)
}

pub fn default_overdue(book: &dyn GateBook, now_ms: u64, deliver: impl Fn(&str, &str) -> NorthResult<()>) -> NorthResult<Vec<(i64, NorthResult<Closure>)>> {
    let overdue = book.gates(now_ms)?.into_iter().filter(|gate| due(gate, now_ms));
    let settled = overdue.map(|gate| (gate.id, super::settle::settle(book, gate.id, Decision::Default, now_ms, &deliver)));
    Ok(settled.filter(|(_, outcome)| !matches!(outcome, Err(NorthError::GateAnswered { .. }))).collect())
}
