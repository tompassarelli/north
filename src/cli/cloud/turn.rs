//! A cloud session's turn: polling it to its end, spotting a stall, nudging it, sweeping sessions
//! waiting on a human, and recording the row of a launch that finished.
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use crate::foundation::clock::Timestamp;

use super::api::{Api, events, field, pages, request, save};

pub(super) const POLL: Duration = Duration::from_secs(30);
pub(super) const NUDGES: usize = 2;
const REPORTS: [&str; 4] = ["Done:", "Not done:", "Split:", "Blocked:"];
const CONTINUE: &str = "Your turn ended while a background run was still going, with no Done:/Not done:/Split: report and no patch handback. Run that work in the foreground now, finish the box, then hand back the patch with a Done:/Not done:/Split: report.";

pub(super) fn sweep(api: &mut Api, dir: &Path, out: &mut dyn Write) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let mut found = false;
    for session in pages(api, "/v1/sessions?beta=true&limit=100")? {
        if field(&session, "session_status") != "requires_action"
            || field(&session, "environment_kind") != "anthropic_cloud"
        {
            continue;
        }
        found = true;
        let id = field(&session, "id");
        let transcript = dir.join(format!("{id}.jsonl"));
        save(&events(api, id)?, &transcript)?;
        request(
            api,
            "POST",
            &format!("/v1/sessions/{id}/archive?beta=true"),
            Some("{}"),
        )?;
        let _ = writeln!(
            out,
            "stopped {id} ({}): it was waiting on a human; transcript {}",
            field(&session, "title"),
            transcript.display()
        );
    }
    if !found {
        let _ = writeln!(out, "no cloud session is waiting on a human");
    }
    Ok(())
}

fn send_body(id: &str, uuid: &str, text: &str) -> Value {
    json!({"events": [{
        "uuid": uuid,
        "session_id": id,
        "type": "user",
        "parent_tool_use_id": null,
        "message": {"role": "user", "content": text},
    }]})
}

pub(super) fn send(api: &mut Api, id: &str, uuid: &str, text: &str) -> Result<u16, String> {
    let body = send_body(id, uuid, text).to_string();
    api(
        "POST",
        &format!("/v1/sessions/{id}/events?beta=true"),
        Some(&body),
    )
    .map(|(status, _)| status)
}

pub(in crate::cli) fn turn_ended(events: &[Value], after: Option<&str>) -> bool {
    let start = match after {
        Some(uuid) => match events.iter().position(|event| field(event, "uuid") == uuid) {
            Some(index) => index + 1,
            None => return false,
        },
        None => 0,
    };
    let last = events[start..]
        .iter()
        .rev()
        .find(|event| matches!(field(event, "type"), "assistant" | "user" | "result"));
    match last.map(|event| field(event, "type")) {
        Some("result") => true,
        Some("assistant") => last
            .and_then(|event| event.get("message"))
            .is_some_and(|message| field(message, "stop_reason") == "end_turn"),
        _ => false,
    }
}

fn finished(api: &mut Api, id: &str, after: Option<&str>) -> Result<Option<Vec<Value>>, String> {
    let session = request(api, "GET", &format!("/v1/sessions/{id}?beta=true"), None)?;
    if field(&session, "session_status") != "idle" {
        return Ok(None);
    }
    let events = events(api, id)?;
    Ok(turn_ended(&events, after).then_some(events))
}

pub(super) fn wait(
    api: &mut Api,
    id: &str,
    out: &Path,
    after: Option<&str>,
    polls: u64,
    sleep: &mut dyn FnMut(),
) -> Result<Option<Vec<Value>>, String> {
    for poll in 0..=polls {
        if poll > 0 {
            sleep();
        }
        match finished(api, id, after) {
            Ok(Some(events)) => return save(&events, out).map(|()| Some(events)),
            Ok(None) => {}
            Err(error) => eprintln!("north cloud wait: {error}"),
        }
    }
    Ok(None)
}

fn since<'a>(events: &'a [Value], after: Option<&str>) -> &'a [Value] {
    let start = after
        .and_then(|uuid| events.iter().position(|event| field(event, "uuid") == uuid))
        .map_or(0, |index| index + 1);
    &events[start..]
}

fn reply_text(event: &Value) -> String {
    match event.pointer("/message/content") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter(|block| field(block, "type") == "text")
            .map(|block| field(block, "text"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn reports(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line
            .trim_start_matches(|c: char| c.is_whitespace() || c == '*' || c == '_' || c == '#');
        REPORTS.iter().any(|report| line.starts_with(report))
    })
}

pub(super) fn stalled(events: &[Value], after: Option<&str>) -> bool {
    let turn = since(events, after);
    let last_reply = turn
        .iter()
        .rev()
        .filter(|event| field(event, "type") == "assistant")
        .map(reply_text)
        .find(|text| !text.trim().is_empty())
        .unwrap_or_default();
    !reports(&last_reply) && !crate::cli::cloud_handback::has_handback(turn)
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Finish {
    Ended(usize),
    Stalled(usize),
    TimedOut,
}

pub(super) struct Waker<'a> {
    pub(super) polls: u64,
    pub(super) sleep: &'a mut dyn FnMut(),
    pub(super) uuid: &'a mut dyn FnMut() -> Result<String, String>,
    pub(super) report: &'a mut dyn Write,
}

pub(super) fn finish(
    api: &mut Api,
    id: &str,
    out: &Path,
    after: Option<&str>,
    waker: Waker,
) -> Result<Finish, String> {
    let mut after = after.map(str::to_owned);
    let mut nudges = 0;
    loop {
        let Some(events) = wait(api, id, out, after.as_deref(), waker.polls, waker.sleep)? else {
            return Ok(Finish::TimedOut);
        };
        let stall = stalled(&events, after.as_deref());
        if !stall || nudges == NUDGES {
            return Ok(if stall {
                Finish::Stalled(events.len())
            } else {
                Finish::Ended(events.len())
            });
        }
        let uuid = (waker.uuid)()?;
        let status = send(api, id, &uuid, CONTINUE)?;
        if !(200..300).contains(&status) {
            return Err(format!(
                "posting the continue message to {id}: HTTP {status}"
            ));
        }
        let _ = writeln!(
            waker.report,
            "{id} ended its turn with no report or handback; posted continue message {uuid}"
        );
        after = Some(uuid);
        nudges += 1;
    }
}

/// The finished row of a session `north cloud launch` started: from its launch to now, with its last reply as the report.
pub(super) fn finished_row(
    id: &str,
    events: &[Value],
    launches: &[crate::dispatch::CloudLaunch],
    now: Timestamp,
) -> Option<Value> {
    let launch = launches.iter().rev().find(|launch| launch.session == id)?;
    let report = events
        .iter()
        .rev()
        .filter(|event| field(event, "type") == "assistant")
        .map(reply_text)
        .find(|text| !text.trim().is_empty())
        .unwrap_or_default();
    Some(crate::providers::claude::runs::cloud::finished(
        id,
        Timestamp::from_unix_seconds_f64(launch.at),
        now,
        &report,
    ))
}

pub(super) fn record_finished(id: &str, saved: &Path) {
    let events: Vec<Value> = std::fs::read_to_string(saved)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let Some(row) = finished_row(
        id,
        &events,
        &crate::dispatch::cloud_launches(),
        Timestamp::now(),
    ) else {
        return;
    };
    if let Err(error) = crate::threads::client::ingest(format!("{row}\n").as_bytes()) {
        eprintln!("north cloud wait: the run row for {id} went unrecorded: {error}");
    }
}
