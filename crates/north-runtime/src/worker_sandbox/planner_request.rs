//! The planning call of `north work split` and `north graph split` for a confined caller (north#1599): a worker's domain reads no model account's login, so the one-shot call the planner makes has no subscription to ask. The request is the words `planner-ask MODEL EFFORT SYSTEM SECONDS` and the planning input on the caller's standard input. The broker checks the words against the asking session's own record (north-fleet-store), then runs `north check oneshot` outside every domain with its own environment, pinned to the session's own account, and relays only the answer, as JSON with its token counts, and the exit code. The request names no program or path the broker would run.

use north_dispatch::router::tier::family_of;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;

use serde_json::Value;

pub(super) const VERB: &str = "planner-ask";
pub(super) const SECONDS_CAP: u64 = 900;
pub(super) const INPUT_CAP: usize = 256 << 10;
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// Whether `arguments` have the shape of a planning call, whatever the session that asks.
pub(super) fn named(arguments: &[String]) -> bool {
    matches!(arguments, [verb, ..] if verb == VERB) && arguments.len() == 5
}

/// The `check oneshot` words the broker starts for the planning call `arguments` name from the asking session `record`: the model must be in the session's own tier, the effort no higher than the session's own, the seconds at most `SECONDS_CAP`, and the call is pinned to the session's own account.
pub(super) fn admitted(arguments: &[String], record: &Value) -> Result<Vec<String>, String> {
    let [_, model, effort, system, seconds] = arguments else { return Err("a planning call carries four words after its verb".to_owned()) };
    let seconds: u64 = seconds.parse().map_err(|_| format!("the planning call's seconds {seconds} is not a whole number"))?;
    if seconds > SECONDS_CAP {
        return Err(format!("the planning call asks {seconds} s, over the {SECONDS_CAP} s cap"));
    }
    if !name(model) {
        return Err(format!("{model} is not a model name"));
    }
    let tier = record["model"].as_str().and_then(family_of).ok_or("the asking session records no model tier")?;
    if family_of(model) != Some(tier) {
        return Err(format!("the model {model} is not in the asking session's {tier} tier"));
    }
    let rank = |word: &str| EFFORTS.iter().position(|level| *level == word);
    let asked = rank(effort).ok_or_else(|| format!("{effort} is not an effort"))?;
    let own = record["effort"].as_str().and_then(rank).ok_or("the asking session records no effort")?;
    if asked > own {
        return Err(format!("the effort {effort} is above the asking session's own {}", EFFORTS[own]));
    }
    let account = record["account"].as_str().filter(|account| name(account)).ok_or("the asking session records no account")?;
    let session = record["name"].as_str().filter(|session| name(session)).ok_or("the asking session records no name")?;
    Ok(["check", "oneshot", "--model", model, "--effort", effort, "--system", system, "--seconds", &seconds.to_string(), "--json", "--account", account, "--session", session].map(str::to_owned).to_vec())
}

/// The words of the planning call `arguments` name, admitted from the asking session `record`, and the planning input on `stdin`, which no longer than `INPUT_CAP`; each refusal is written to `stderr` and returned.
pub(super) fn request(arguments: &[String], record: &Value, stdin: OwnedFd, stderr: &mut impl Write) -> Result<(Vec<String>, Vec<u8>), String> {
    let words = admitted(arguments, record).map_err(|why| refusal(stderr, why))?;
    let mut input = Vec::new();
    File::from(stdin).take(INPUT_CAP as u64 + 1).read_to_end(&mut input).map_err(|error| error.to_string())?;
    if input.len() > INPUT_CAP {
        return Err(refusal(stderr, format!("the planning input is over {} KiB", INPUT_CAP >> 10)));
    }
    Ok((words, input))
}

fn refusal(stderr: &mut impl Write, why: String) -> String {
    let _ = writeln!(stderr, "north worker-sandbox broker: refused: {why}");
    why
}

fn name(word: &str) -> bool {
    word.len() <= 64 && word.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric) && word.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}
