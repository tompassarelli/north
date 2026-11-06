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
        let scoped_flags = [&flags[..], &["--issue", "north#370", "--box", "ring crate"]].concat();
        assert_eq!(gate_request(&scoped_flags).unwrap().box_text.as_deref(), Some("ring crate"));
        assert!(gate_request(&[&flags[..], &["--box", "ring crate"]].concat()).is_err(), "a --box without an --issue scopes nothing");
        assert!(gate_request(&[&flags[..], &["--issue", "north#370", "--box", " "]].concat()).is_err(), "an empty --box must be refused");
        let Err(NorthError::Usage(help)) = gate_request(&["--help"]) else { panic!("--help prints the usage") };
        assert!(help.contains("--box TEXT"), "{help}");
        let ask = ["--session", "w", "--question", "Encrypt the GitHub App credential to one more host?", "--option", "yes", "--option", "no", "--recommend", "1"];
        for defaults in [&["--default", "1", "--default-after", "4h"][..], &["--default", "1"], &["--default-after", "4h"]] {
            let Err(NorthError::Usage(refusal)) = gate_request(&[&ask[..], defaults].concat()) else { panic!("an ask-once gate with {defaults:?} must be refused") };
            assert!(refusal.contains("ask-once matter (credentials)"), "{refusal}");
        }
        let request = gate_request(&ask).unwrap();
        let gate = Gate { id: 3, request, opened_ms: 1_000, closure: None };
        let rows = listed("laptop", std::slice::from_ref(&gate), &[]);
        assert_eq!((rows[0]["question"].as_str(), rows[0]["default"].is_null(), rows[0]["default_at_ms"].is_null()), (Some("Encrypt the GitHub App credential to one more host?"), true, true));
        assert!(!due(&gate, u64::MAX), "a gate with no default never falls due");
        let plain = ["--session", "w", "--question", "Rename the module?", "--option", "yes", "--option", "no", "--recommend", "1"];
        assert!(gate_request(&plain).is_err(), "a question outside the ask-once table keeps its required default");
        let request = |box_text: Option<&str>| GateRequest { verification: false, issue: Some("north#657".into()), box_text: box_text.map(str::to_owned), session: "w".into(), question: "Rename?".into(), options: vec!["yes".into(), "no".into()], recommend: 1, default: 1, default_after_ms: 60_000 };
        let Err(NorthError::Usage(refusal)) = scoped(&request(None), |_| Ok(3)) else { panic!("a gate without --box on a 3-box issue must be refused") };
        assert!(refusal.contains("north#657 has 3 open boxes"), "{refusal}");
        assert!(scoped(&request(Some("scoreboard")), |_| panic!("a gate naming its box counts nothing")).is_ok());
        assert!(scoped(&request(None), |_| Ok(1)).is_ok(), "a single-box issue is held whole");
        let unscoped = GateRequest { issue: None, ..request(None) };
        assert!(scoped(&unscoped, |_| panic!("a gate on no issue reads no issue")).is_ok());
        let options = vec!["yes".to_owned(), "no".to_owned()];
        let class = |question: &str| crate::fleet::ask_once::class(question, &options);
        assert_eq!([class("Widen access to gex45 for the app?"), class("Delete the ledger?")], [Some("credentials"), Some("deletion")]);
        for routine in ["Delete the dead test helper?", "Which account should the retry run on?", "Make `parse` public?", "Raise the token budget to 350k?", "Is the CPU cost acceptable?", "Is the accountant happy?"] {
            assert_eq!(class(routine), None, "{routine} is a routine call that keeps its default");
        }
    }
}
