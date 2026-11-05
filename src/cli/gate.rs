use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};
use crate::fleet::gates::{Decision, Gate, GateRequest, due};
use crate::fleet::registry::{Fleet, Opened};
use crate::fleet::runtime::now_ms;
use crate::fleet::sender::Sender;
use crate::fleet::store::GateRow;
use crate::dispatch::IssueRef;

const USAGE: &str = "Usage: north gate open --session NAME --question TEXT --option TEXT --option TEXT... --recommend N --default N --default-after DURATION (30s, 10m, 4h, 1d) [--issue OWNER/REPO#N [--box TEXT]] | north gate answer ID|HOST:ID N [--note TEXT] [--session NAME] | north gate list [--json]";
const BOX_HELP: &str = "--box TEXT scopes the --issue hold to the one box whose `Item:` line carries TEXT, so launches for the issue's other boxes go ahead; without --box the gate holds the whole issue.";
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
    let caller = fleet.caller();
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
    NorthError::Usage(format!("{USAGE}\n{BOX_HELP}"))
}

fn open(fleet: &Fleet, flags: &[&str]) -> NorthResult<()> {
    let request = gate_request(flags)?;
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
    if request.session.is_empty() || request.default_after_ms == 0 {
        return Err(usage());
    }
    request.validate()?;
    Ok(request)
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
        "default": gate.request.default,
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
        "default": gate.request.default,
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
    fn the_list_carries_this_hosts_gates_beside_the_open_gates_other_hosts_hold_with_their_answerable_ids() {
        let request = |session: &str, issue: Option<&str>| GateRequest { verification: false, issue: issue.map(str::to_owned), box_text: None, session: session.into(), question: "Rent the box?".into(), options: vec!["yes".into(), "no".into()], recommend: 1, default: 2, default_after_ms: 600_000 };
        let mine = Gate { id: 7, request: request("proxy", None), opened_ms: 1_000, closure: None };
        let theirs = GateRow { host: "nexus".into(), id: 4, request: request("fdl-probe", Some("north#416")), opened_ms: 2_000 };

        let rows = listed("laptop", &[mine], &[theirs]);

        let seen: Vec<(&str, String, &str)> = rows.iter().map(|row| (row["host"].as_str().unwrap(), row["id"].to_string(), row["session"].as_str().unwrap())).collect();
        assert_eq!(seen, [("laptop", "7".to_owned(), "proxy"), ("nexus", "\"nexus:4\"".to_owned(), "fdl-probe")]);
        assert_eq!((rows[1]["issue"].as_str(), rows[1]["default_at_ms"].as_u64(), rows[1]["closed_ms"].is_null()), (Some("north#416"), Some(602_000), true));
    }

    #[test]
    fn open_reads_repeated_options_and_a_unit_duration() {
        let flags = ["--session", "w", "--question", "Rent?", "--option", "yes", "--option", "no", "--recommend", "1", "--default", "2", "--default-after", "30s"];
        let request = gate_request(&flags).unwrap();
        assert_eq!((request.options.as_slice(), request.recommend, request.default, request.default_after_ms), (&["yes".to_owned(), "no".to_owned()][..], 1, 2, 30_000));
        assert_eq!([duration_ms("10m"), duration_ms("4h"), duration_ms("1d"), duration_ms("5"), duration_ms("h")], [Some(600_000), Some(14_400_000), Some(86_400_000), None, None]);
        assert!(gate_request(&flags[..flags.len() - 2]).is_err(), "a gate without --default-after must be refused");
        let decided = [&flags[..], &["--issue", "tompassarelli/North#370"]].concat();
        assert_eq!(gate_request(&decided).unwrap().issue.as_deref(), Some("north#370"));
        assert!(gate_request(&[&flags[..], &["--issue", "370"]].concat()).is_err(), "an --issue without a repository must be refused");
        let scoped = [&flags[..], &["--issue", "north#370", "--box", "ring crate"]].concat();
        assert_eq!(gate_request(&scoped).unwrap().box_text.as_deref(), Some("ring crate"));
        assert!(gate_request(&[&flags[..], &["--box", "ring crate"]].concat()).is_err(), "a --box without an --issue scopes nothing");
        assert!(gate_request(&[&flags[..], &["--issue", "north#370", "--box", " "]].concat()).is_err(), "an empty --box must be refused");
        let Err(NorthError::Usage(help)) = gate_request(&["--help"]) else { panic!("--help prints the usage") };
        assert!(help.contains("--box TEXT"), "{help}");
    }
}
