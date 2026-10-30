use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};
use crate::fleet::gates::{Decision, Gate, GateRequest, due};
use crate::fleet::registry::{Fleet, Opened};
use crate::fleet::relay::Ssh;
use crate::fleet::runtime::now_ms;

const USAGE: &str = "Usage: north gate open --session NAME --question TEXT --option TEXT --option TEXT... --recommend N --default N --default-after DURATION (30s, 10m, 4h, 1d) | north gate answer ID|HOST:ID N [--note TEXT] | north gate list [--json]";
const WAIT_POLL: Duration = Duration::from_secs(2);

pub fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    match words {
        ["open", flags @ ..] => open(fleet, flags),
        ["answer", id, number] => answer(fleet, id, number, None),
        ["answer", id, number, "--note", note] => answer(fleet, id, number, Some(note)),
        ["list"] => list(fleet, false),
        ["list", "--json"] => list(fleet, true),
        ["await", id] => await_default(fleet, id.parse().map_err(|_| usage())?),
        _ => Err(usage()),
    }
}

fn answer(fleet: &Fleet, id: &str, number: &str, note: Option<&str>) -> NorthResult<()> {
    let caller = fleet.caller();
    if id.contains(':') {
        println!("{}", fleet.answer_remote_gate(&Ssh, id, number.parse().map_err(|_| usage())?, note, caller.as_deref())?);
        return Ok(());
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
    NorthError::Usage(USAGE.into())
}

fn open(fleet: &Fleet, flags: &[&str]) -> NorthResult<()> {
    let request = gate_request(flags)?;
    match fleet.open_or_post_gate(&request)? {
        Opened::Here(id) => println!("{id}"),
        Opened::Posted { relay, letter } => {
            let minutes = relay.age.as_secs() / 60;
            println!("{}", letter.id);
            eprintln!("north gate: no session named {} here; posted {} for {}, which last relayed {minutes}m ago; the gate opens there with its own id", request.session, letter.id, relay.host);
        }
    }
    Ok(())
}

fn gate_request(flags: &[&str]) -> NorthResult<GateRequest> {
    let mut request = GateRequest { verification: false, session: String::new(), question: String::new(), options: Vec::new(), recommend: 0, default: 0, default_after_ms: 0 };
    for pair in flags.chunks(2) {
        let [flag, value] = pair else { return Err(usage()) };
        match *flag {
            "--session" => request.session = (*value).to_owned(),
            "--question" => request.question = (*value).to_owned(),
            "--option" => request.options.push((*value).to_owned()),
            "--recommend" => request.recommend = value.parse().map_err(|_| usage())?,
            "--default" => request.default = value.parse().map_err(|_| usage())?,
            "--default-after" => request.default_after_ms = duration_ms(value).ok_or_else(usage)?,
            _ => return Err(usage()),
        }
    }
    if request.session.is_empty() || request.default_after_ms == 0 {
        return Err(usage());
    }
    request.validate()?;
    Ok(request)
}

fn duration_ms(text: &str) -> Option<u64> {
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
    if json {
        println!("{}", Value::Array(gates.iter().map(gate_json).collect()));
        return Ok(());
    }
    for gate in &gates {
        let state = gate.closure.as_ref().map_or_else(|| "open".to_owned(), |closure| format!("closed by {}: {}", closure.by.label(), closure.answer));
        println!("{}\t{}\t{}\t{state}", gate.id, gate.request.session, gate.request.question);
    }
    for seen in fleet.remote_views() {
        for gate in &seen.snapshot.gates {
            println!("{}\t{}\t{}\topen", seen.qualified(gate), gate.session, gate.question);
        }
    }
    Ok(())
}

fn gate_json(gate: &Gate) -> Value {
    json!({
        "id": gate.id,
        "session": gate.request.session,
        "question": gate.request.question,
        "options": gate.request.options,
        "recommend": gate.request.recommend,
        "default": gate.request.default,
        "opened_ms": gate.opened_ms,
        "default_at_ms": gate.default_at_ms(),
        "answer": gate.closure.as_ref().map(|closure| &closure.answer),
        "answered_by": gate.closure.as_ref().map(|closure| closure.by.label()),
        "closed_ms": gate.closure.as_ref().map(|closure| closure.closed_ms),
    })
}

fn await_default(fleet: &Fleet, id: i64) -> NorthResult<()> {
    loop {
        let Some(gate) = fleet.gate(id)?.filter(Gate::is_open) else { return Ok(()) };
        let now = now_ms();
        if due(&gate, now) {
            return match fleet.decide(id, Decision::Default) {
                Err(NorthError::GateAnswered { .. }) => Ok(()),
                outcome => outcome.map(drop),
            };
        }
        std::thread::sleep(WAIT_POLL.min(Duration::from_millis(gate.default_at_ms() - now)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_reads_repeated_options_and_a_unit_duration() {
        let flags = ["--session", "w", "--question", "Rent?", "--option", "yes", "--option", "no", "--recommend", "1", "--default", "2", "--default-after", "30s"];
        let request = gate_request(&flags).unwrap();
        assert_eq!((request.options.as_slice(), request.recommend, request.default, request.default_after_ms), (&["yes".to_owned(), "no".to_owned()][..], 1, 2, 30_000));
        assert_eq!([duration_ms("10m"), duration_ms("4h"), duration_ms("1d"), duration_ms("5"), duration_ms("h")], [Some(600_000), Some(14_400_000), Some(86_400_000), None, None]);
        assert!(gate_request(&flags[..flags.len() - 2]).is_err(), "a gate without --default-after must be refused");
    }
}
