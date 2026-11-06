use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};
use crate::fleet::gates::{Decision, Gate, GateRequest, due};
use crate::fleet::registry::{Fleet, Opened};
use crate::fleet::runtime::now_ms;
use crate::fleet::sender::Sender;
use crate::fleet::store::GateRow;
use crate::dispatch::{IssueRef, OWNER, product_boxes};
use crate::github_mirror::store::{Mirror, OPEN_ISSUES, key};

const USAGE: &str = "Usage: north gate open --session NAME --question TEXT --option TEXT --option TEXT... --recommend N [--default N --default-after DURATION (30s, 10m, 4h, 1d)] [--issue OWNER/REPO#N [--box TEXT]] | north gate answer ID|HOST:ID N [--note TEXT] [--session NAME] | north gate list [--json]";
const BOX_HELP: &str = "--box TEXT scopes the --issue hold to the one box whose `Item:` line carries TEXT, so launches for the issue's other boxes go ahead; an issue with more than one open box needs --box, and one with a single open box is held whole.";
const DEFAULT_HELP: &str = "--default and --default-after are required, except for a question about cost, accounts, credentials or access, making something public, deleting valuable data, or core direction and dependencies (the ask-once table in harness/defaults.toml): such a gate takes neither and waits for Tom's answer.";
const WAIT_POLL: Duration = Duration::from_secs(2);

pub fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    match words {
        ["open", flags @ ..] => open(fleet, flags),
        ["answer", id, number] => answer(fleet, id, number, None, None),
        ["answer", id, number, "--note", note] => answer(fleet, id, number, Some(note), None),
        ["answer", id, number, "--session", session] => answer(fleet, id, number, None, Some(session)),
        ["answer", id, number, "--note", note, "--session", session] | ["answer", id, number, "--session", session, "--note", note] => answer(fleet, id, number, Some(note), Some(session)),
        ["list"] => list(fleet, false),
        ["list", "--json"] => list(fleet, true),
        ["await", id] => await_default(fleet, id.parse().map_err(|_| usage())?),
        _ => Err(usage()),
    }
}

fn answer(fleet: &Fleet, id: &str, number: &str, note: Option<&str>, session: Option<&str>) -> NorthResult<()> {
    let caller = fleet.answerer();
    if id.contains(':') {
        println!("{}", fleet.answer_remote_gate(id, session, number.parse().map_err(|_| usage())?, note, caller.as_deref())?);
        return Ok(());
    }
    if session.is_some() {
        return Err(usage());
    }
    let result = fleet.answer_gate(id.parse().map_err(|_| usage())?, number.parse().map_err(|_| usage())?, note, caller.as_deref());
    let closure = match result {
        Err(error @ NorthError::GateAnswered { .. }) => { println!("{}", error.user_message()); return Ok(()); }
        result => result?,
    };
    println!("gate {id} closed by {}: {}", closure.by.label(), closure.answer);
    Ok(())
}

fn usage() -> NorthError {
    NorthError::Usage(format!("{USAGE}\n{BOX_HELP}\n{DEFAULT_HELP}"))
}

fn open(fleet: &Fleet, flags: &[&str]) -> NorthResult<()> {
    let request = gate_request(flags)?;
    scoped(&request, open_boxes)?;
    match fleet.open_or_spool_gate(&request, &Sender::typed(fleet.caller()))? {
        Opened::Here(id) => println!("{id}"),
        Opened::Spooled(row) => {
            println!("{}", row.id);
            eprintln!("north gate: no session named {} here; spooled GATE row {} for the host that has it, which opens the gate with its own id", request.session, row.id);
        }
    }
    Ok(())
}

fn gate_request(flags: &[&str]) -> NorthResult<GateRequest> {
    let mut request = GateRequest { verification: false, issue: None, box_text: None, session: String::new(), question: String::new(), options: Vec::new(), recommend: 0, default: 0, default_after_ms: 0 };
    for pair in flags.chunks(2) {
        let [flag, value] = pair else { return Err(usage()) };
        match *flag {
            "--session" => request.session = (*value).to_owned(),
            "--question" => request.question = (*value).to_owned(),
            "--option" => request.options.push((*value).to_owned()),
            "--recommend" => request.recommend = value.parse().map_err(|_| usage())?,
            "--default" => request.default = value.parse().map_err(|_| usage())?,
            "--default-after" => request.default_after_ms = duration_ms(value).ok_or_else(usage)?,
            "--issue" => request.issue = Some(IssueRef::parse(value).ok_or_else(usage)?.to_string()),
            "--box" => request.box_text = Some((*value).to_owned()),
            _ => return Err(usage()),
        }
    }
    if request.session.is_empty() {
        return Err(usage());
    }
    request.validate()?;
    Ok(request)
}

fn scoped(request: &GateRequest, open_boxes: impl FnOnce(&str) -> NorthResult<usize>) -> NorthResult<()> {
    let (Some(issue), None) = (request.issue.as_deref(), request.box_text.as_deref()) else { return Ok(()) };
    match open_boxes(issue)? {
        0 | 1 => Ok(()),
        count => Err(NorthError::Usage(format!("{issue} has {count} open boxes: name the one this gate decides with --box TEXT, or the gate holds the launches for all of them"))),
    }
}

fn open_boxes(issue: &str) -> NorthResult<usize> {
    let reference = IssueRef::parse(issue).ok_or_else(usage)?;
    let mirrored = Mirror::local().fresh(&key(&reference.repo, OPEN_ISSUES), now_ms() as f64 / 1000.0);
    let node = mirrored.as_ref().and_then(Value::as_array).and_then(|nodes| nodes.iter().find(|node| node["number"].as_u64() == Some(reference.number)));
    if let Some(node) = node {
        return Ok(product_boxes(node["body"].as_str().unwrap_or_default()));
    }
    let repo = format!("{OWNER}/{}", reference.repo);
    let found = crate::platform::github::json(&["issue", "view", &reference.number.to_string(), "-R", &repo, "--json", "body"]).map_err(|why| NorthError::Usage(format!("cannot read {issue} to count its open boxes ({}); name the box this gate decides with --box TEXT", why.trim())))?;
    Ok(product_boxes(found["body"].as_str().unwrap_or_default()))
}

pub(crate) fn duration_ms(text: &str) -> Option<u64> {
    let split = text.find(|character: char| !character.is_ascii_digit())?;
    let (count, unit) = text.split_at(split);
    let seconds = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3_600,
        "d" => 86_400,
        _ => return None,
    };
    count.parse::<u64>().ok()?.checked_mul(seconds * 1000)
}

fn list(fleet: &Fleet, json: bool) -> NorthResult<()> {
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

fn await_default(fleet: &Fleet, id: i64) -> NorthResult<()> {
    loop {
        let Some(gate) = fleet.gate(id)?.filter(Gate::is_open) else { return Ok(()) };
        let Some(at) = gate.default_at_ms() else { return Ok(()) };
        let now = now_ms();
        if due(&gate, now) {
            return match fleet.decide(id, Decision::Default) {
                Err(NorthError::GateAnswered { .. }) => Ok(()),
                outcome => outcome.map(drop),
            };
        }
        std::thread::sleep(WAIT_POLL.min(Duration::from_millis(at - now)));
    }
}
