use serde_json::{Value, json};

use crate::error::NorthResult;
use crate::fleet::gates::Gate;
use crate::fleet::registry::Fleet;
use crate::fleet::store::GateRow;

pub(super) fn list(fleet: &Fleet, json: bool) -> NorthResult<()> {
    let gates = fleet.digest(0)?.gates;
    let elsewhere = fleet.gates_elsewhere()?;
    if json {
        println!("{}", Value::Array(listed(fleet.host(), &gates, &elsewhere)));
        return Ok(());
    }
    for gate in &elsewhere {
        println!("{}\t{}\t{}\topen", gate.label(), gate.request.session, gate.request.question);
    }
    for gate in &gates {
        let state = gate.closure.as_ref().map_or_else(|| "open".to_owned(), |closure| format!("closed by {}: {}", closure.by.label(), closure.answer));
        println!("{}\t{}\t{}\t{state}", gate.id, gate.request.session, gate.request.question);
    }
    Ok(())
}

pub(super) fn list_aside(fleet: &Fleet, json: bool) -> NorthResult<()> {
    let gates = fleet.aside_gates()?;
    if json {
        println!("{}", Value::Array(gates.iter().map(|gate| gate_json(gate, fleet.host())).collect()));
        return Ok(());
    }
    for gate in &gates {
        let state = gate.closure.as_ref().map_or_else(|| "open".to_owned(), |closure| format!("closed by {}: {}", closure.by.label(), closure.answer));
        println!("{}\t{}\t{}\t{state}", gate.id, gate.request.session, gate.request.question);
    }
    Ok(())
}

fn listed(host: &str, gates: &[Gate], elsewhere: &[GateRow]) -> Vec<Value> {
    gates.iter().map(|gate| gate_json(gate, host)).chain(elsewhere.iter().map(row_json)).collect()
}

fn gate_json(gate: &Gate, host: &str) -> Value {
    json!({
        "host": host,
        "id": gate.id,
        "session": gate.request.session,
        "question": gate.request.question,
        "issue": gate.request.issue,
        "box": gate.request.box_text,
        "options": gate.request.options,
        "recommend": gate.request.recommend,
        "default": gate.request.has_default().then_some(gate.request.default),
        "opened_ms": gate.opened_ms,
        "default_at_ms": gate.default_at_ms(),
        "answer": gate.closure.as_ref().map(|closure| &closure.answer),
        "answered_by": gate.closure.as_ref().map(|closure| closure.by.label()),
        "closed_ms": gate.closure.as_ref().map(|closure| closure.closed_ms),
    })
}

fn row_json(gate: &GateRow) -> Value {
    json!({
        "host": gate.host,
        "id": gate.label(),
        "session": gate.request.session,
        "question": gate.request.question,
        "issue": gate.request.issue,
        "box": gate.request.box_text,
        "options": gate.request.options,
        "recommend": gate.request.recommend,
        "default": gate.request.has_default().then_some(gate.request.default),
        "opened_ms": gate.opened_ms,
        "default_at_ms": gate.default_at_ms(),
        "answer": null,
        "answered_by": null,
        "closed_ms": null,
    })
}

