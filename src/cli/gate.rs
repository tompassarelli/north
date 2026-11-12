use std::time::Duration;

use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::fleet::decision_queue;
use crate::fleet::gates::{Decision, Gate, GateRequest, due};
use crate::fleet::registry::{Fleet, Opened};
use crate::fleet::runtime::now_ms;
use crate::fleet::sender::Sender;
use crate::dispatch::{IssueRef, OWNER, product_boxes};
use crate::github_mirror::store::{Mirror, OPEN_ISSUES, key};
use crate::platform::attribution::{self, Cause};
use north_guard::guard::break_glass;
use super::gate_list::{list, list_aside};

const USAGE: &str = "Usage: north gate open [--session NAME (default $NORTH_FLEET_SESSION)] --question TEXT --option TEXT --option TEXT... --recommend N [--default N --default-after DURATION (30s, 10m, 4h, 1d) | --ask-once CATEGORY] [--issue OWNER/REPO#N [--box TEXT]] | north gate answer ID|HOST:ID N [--note TEXT] [--session NAME] | north gate answer ID N --relayed-from-turn CONVO_ID | north gate list [--json] | north gate list --aside [--json] | north gate digest [--peek]";
const BOX_HELP: &str = "--box TEXT scopes the --issue hold to the one box whose `Item:` line carries TEXT, so launches for the issue's other boxes go ahead; an issue with more than one open box needs --box, and one with a single open box is held whole.";
const DEFAULT_HELP: &str = "--default and --default-after are required unless the gate names the ask-once category of its recommended option with --ask-once CATEGORY (cost, accounts, credentials, public, deletion, tom_name or direction, the ask-once table in harness/defaults.toml): such a gate takes neither and is for Tom: only the work it holds waits for his answer. Every other gate applies its recommended default at its deadline (8h unless the brief names one).";
const FIRST_LOOK_MS: u64 = 24 * 3_600_000;
const WAIT_POLL: Duration = Duration::from_secs(2);

pub fn run(fleet: &Fleet, words: &[&str]) -> NorthResult<()> {
    match words {
        ["open", flags @ ..] => open(fleet, flags),
        ["answer", id, number, flags @ ..] => answer(fleet, id, number, flags),
        ["list", "--aside"] => list_aside(fleet, false),
        ["list", "--aside", "--json"] => list_aside(fleet, true),
        ["list"] => list(fleet, false),
        ["list", "--json"] => list(fleet, true),
        ["digest"] => digest(fleet, true),
        ["digest", "--peek"] => digest(fleet, false),
        ["await", id] => await_default(fleet, id.parse().map_err(|_| usage())?),
        _ => Err(usage()),
    }
}

fn answer(fleet: &Fleet, id: &str, number: &str, flags: &[&str]) -> NorthResult<()> {
    let (mut note, mut session, mut turn) = (None, None, None);
    for pair in flags.chunks(2) {
        let [flag, value] = pair else { return Err(usage()) };
        match *flag {
            "--note" => note = Some(*value),
            "--session" => session = Some(*value),
            "--relayed-from-turn" => turn = Some(value.parse::<i64>().map_err(|_| usage())?),
            _ => return Err(usage()),
        }
    }
    let number = number.parse().map_err(|_| usage())?;
    if let Some(turn) = turn {
        if id.contains(':') || session.is_some() || note.is_some() {
            return Err(usage());
        }
        println!("{}", super::gate_relay::relay(fleet, id.parse().map_err(|_| usage())?, number, turn)?);
        return Ok(());
    }
    let caller = fleet.answerer();
    if id.contains(':') {
        println!("{}", fleet.answer_remote_gate(id, session, number, note, caller.as_deref())?);
        return Ok(());
    }
    if session.is_some() {
        return Err(usage());
    }
    let typed = super::fleet::proxy_turn(fleet, caller.as_deref());
    let result = fleet.answer_gate(id.parse().map_err(|_| usage())?, number, note, caller.as_deref(), typed.as_ref());
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
    let opened = fleet.open_or_spool_gate(&request, &Sender::typed(fleet.caller()))?;
    attribution::record(&request.session, "gate", Cause::Tom, &request.question);
    match opened {
        Opened::Here(id) => println!("{id}"),
        Opened::Spooled(row) => {
            println!("{}", row.id);
            eprintln!("north gate: no session named {} here; spooled GATE row {} for the host that has it, which opens the gate with its own id", request.session, row.id);
        }
    }
    Ok(())
}

fn gate_request(flags: &[&str]) -> NorthResult<GateRequest> {
    let mut request = GateRequest { verification: false, issue: None, box_text: None, session: String::new(), question: String::new(), options: Vec::new(), recommend: 0, default: 0, default_after_ms: 0, ask_once: None };
    for pair in flags.chunks(2) {
        let [flag, value] = pair else { return Err(usage()) };
        match *flag {
            "--session" => request.session = (*value).to_owned(),
            "--question" => request.question = (*value).to_owned(),
            "--option" => request.options.push((*value).to_owned()),
            "--recommend" => request.recommend = value.parse().map_err(|_| usage())?,
            "--default" => request.default = value.parse().map_err(|_| usage())?,
            "--default-after" => request.default_after_ms = duration_ms(value).ok_or_else(usage)?,
            "--ask-once" => request.ask_once = Some((*value).to_owned()),
            "--issue" => request.issue = Some(IssueRef::parse(value).ok_or_else(usage)?.to_string()),
            "--box" => request.box_text = Some((*value).to_owned()),
            _ => return Err(usage()),
        }
    }
    if request.session.is_empty() {
        request.session = std::env::var(crate::fleet::runtime::SESSION_ENV).unwrap_or_default();
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

fn digest(fleet: &Fleet, mark: bool) -> NorthResult<()> {
    let seen = std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/state/north/gate-digest/seen"));
    let since = seen.as_ref().and_then(|path| std::fs::read_to_string(path).ok()).and_then(|text| text.trim().parse().ok()).unwrap_or_else(|| now_ms().saturating_sub(FIRST_LOOK_MS));
    let now = now_ms();
    print!("{}", decision_queue::digest(&fleet.digest(since)?.gates, &fleet.gates_elsewhere()?, since, now));
    print!("{}", break_glass::digest_section(&break_glass::recorded(), since, now));
    if let Some(path) = seen.filter(|_| mark) {
        let _ = path.parent().map(std::fs::create_dir_all);
        std::fs::write(&path, now.to_string()).map_err(|why| NorthError::State(format!("cannot record the digest look in {}: {why}", path.display())))?;
    }
    Ok(())
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
