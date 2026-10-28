use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::{Value, json};

const USAGE: &str = "usage: north cloud list | launch REPO SLUG BRIEF [--item REPO#N] | events SESSION_ID OUT.jsonl | sweep OUT_DIR | send SESSION_ID FILE | wait SESSION_ID OUT.jsonl [--after UUID] [--timeout SECS] | collect EVENTS.jsonl OUT.mbox
launch refreshes /tmp/cloud/src/REPO, trusts it, starts BRIEF with its landing steps through claude --cloud and prints the session, claude/land-SLUG and the checkout; --item records the launch so north dispatch holds that issue for six hours.
sweep archives only cloud-run sessions (environment_kind anthropic_cloud) waiting on a human, never Remote Control bridges.
send posts FILE as a user message to the session and prints the HTTP status and the message's uuid.
wait returns once the session is idle and its latest turn ended, after the message UUID when --after is given, then writes its events (default timeout 3600 s; exit 1 on timeout).
collect decodes the latest multi-part patch handback in a saved events transcript to an mbox, reading no session and needing no login; a missing part, a failed decode or output not starting with From is refused.
Run on the host whose ~/.claude login owns the cloud sessions (south:agents/skills/claude-accounts/).";

const BASE: &str = "https://api.anthropic.com";
const POLL: Duration = Duration::from_secs(30);

pub(super) type Api<'a> = dyn FnMut(&str, &str, Option<&str>) -> Result<(u16, String), String> + 'a;

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Cloud {
    List,
    Events(String, PathBuf),
    Sweep(PathBuf),
    Send(String, PathBuf),
    Wait(String, PathBuf, u64, Option<String>),
    Collect(PathBuf, PathBuf),
    Launch(String, String, PathBuf, Option<String>),
}

pub(super) fn parse(arguments: &[String]) -> Option<Cloud> {
    let words = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    Some(match words.as_slice() {
        ["list"] => Cloud::List,
        ["launch", repo, slug, brief] => Cloud::Launch((*repo).to_owned(), (*slug).to_owned(), brief.into(), None),
        ["launch", repo, slug, brief, "--item", item] => Cloud::Launch((*repo).to_owned(), (*slug).to_owned(), brief.into(), Some((*item).to_owned())),
        ["events", id, out] => Cloud::Events((*id).to_owned(), out.into()),
        ["collect", events, out] => Cloud::Collect(events.into(), out.into()),
        ["sweep", dir] => Cloud::Sweep(dir.into()),
        ["send", id, file] => Cloud::Send((*id).to_owned(), file.into()),
        ["wait", id, out, options @ ..] => wait_command(id, out, options)?,
        _ => return None,
    })
}

fn wait_command(id: &str, out: &str, options: &[&str]) -> Option<Cloud> {
    let (mut after, mut timeout) = (None, 3600);
    for pair in options.chunks(2) {
        match pair {
            ["--after", uuid] => after = Some((*uuid).to_owned()),
            ["--timeout", secs] => timeout = secs.parse().ok()?,
            _ => return None,
        }
    }
    Some(Cloud::Wait(id.to_owned(), out.into(), timeout, after))
}

pub(super) struct Curl {
    pub(super) base: String,
    pub(super) token: String,
    pub(super) org: String,
}

pub(super) fn curl_config(token: &str, org: &str) -> String {
    let quote = |text: String| {
        format!(
            "header = \"{}\"\n",
            text.replace('\\', "\\\\").replace('"', "\\\"")
        )
    };
    [
        format!("Authorization: Bearer {token}"),
        "anthropic-version: 2023-06-01".to_owned(),
        "anthropic-beta: ccr-byoc-2025-07-29".to_owned(),
        format!("x-organization-uuid: {org}"),
        "content-type: application/json".to_owned(),
    ]
    .into_iter()
    .map(quote)
    .collect()
}

pub(super) fn curl_args(method: &str, url: &str, body: Option<&Path>) -> Vec<String> {
    let mut args = [
        "--silent",
        "--show-error",
        "--max-time",
        "60",
        "--config",
        "-",
        "--request",
        method,
        "--write-out",
        "\n%{http_code}",
    ]
    .map(str::to_owned)
    .to_vec();
    if let Some(body) = body {
        args.extend(["--data-binary".to_owned(), format!("@{}", body.display())]);
    }
    args.push(url.to_owned());
    args
}

impl Curl {
    pub(super) fn call(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<(u16, String), String> {
        let file = body
            .map(|body| {
                let mut file = tempfile::NamedTempFile::new()?;
                file.write_all(body.as_bytes())?;
                Ok::<_, std::io::Error>(file)
            })
            .transpose()
            .map_err(|error| format!("curl body: {error}"))?;
        let mut child = Command::new("curl")
            .args(curl_args(
                method,
                &format!("{}{path}", self.base),
                file.as_ref().map(|file| file.path()),
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("curl: {error}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(curl_config(&self.token, &self.org).as_bytes())
                .map_err(|error| format!("curl: {error}"))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| format!("curl: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "{method} {path}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let (body, status) = text
            .rsplit_once('\n')
            .ok_or_else(|| format!("{method} {path}: no HTTP status"))?;
        let status = status
            .trim()
            .parse()
            .map_err(|_| format!("{method} {path}: bad HTTP status {status}"))?;
        Ok((status, body.to_owned()))
    }
}

fn request(api: &mut Api, method: &str, path: &str, body: Option<&str>) -> Result<Value, String> {
    let (status, text) = api(method, path, body)?;
    if !(200..300).contains(&status) {
        return Err(format!("{method} {path}: HTTP {status}: {text}"));
    }
    serde_json::from_str(&text).map_err(|error| format!("{method} {path}: {error}"))
}

fn pages(api: &mut Api, path: &str) -> Result<Vec<Value>, String> {
    let mut items = Vec::new();
    let mut after = String::new();
    loop {
        let page = request(api, "GET", &format!("{path}{after}"), None)?;
        items.extend(
            page.get("data")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );
        match page.get("last_id").and_then(Value::as_str) {
            Some(last) if page.get("has_more") == Some(&Value::Bool(true)) => {
                after = format!("&after_id={last}")
            }
            _ => return Ok(items),
        }
    }
}

fn field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

pub(super) fn row(session: &Value) -> String {
    [
        "id",
        "session_status",
        "environment_kind",
        "updated_at",
        "title",
    ]
    .map(|key| field(session, key).replace(['\t', '\n'], " "))
    .join("\t")
}

pub(super) fn events(api: &mut Api, id: &str) -> Result<Vec<Value>, String> {
    pages(
        api,
        &format!("/v1/sessions/{id}/events?beta=true&limit=1000"),
    )
}

fn save(events: &[Value], out: &Path) -> Result<(), String> {
    let error = |error: std::io::Error| format!("{}: {error}", out.display());
    let mut file = BufWriter::new(File::create(out).map_err(error)?);
    for event in events {
        writeln!(file, "{event}").map_err(error)?;
    }
    file.flush().map_err(error)
}

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

pub(super) fn send_body(id: &str, uuid: &str, text: &str) -> Value {
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

pub(super) fn turn_ended(events: &[Value], after: Option<&str>) -> bool {
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
) -> Result<Option<usize>, String> {
    for poll in 0..=polls {
        if poll > 0 {
            sleep();
        }
        match finished(api, id, after) {
            Ok(Some(events)) => return save(&events, out).map(|()| Some(events.len())),
            Ok(None) => {}
            Err(error) => eprintln!("north cloud wait: {error}"),
        }
    }
    Ok(None)
}

fn credentials() -> Result<Curl, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME is not set")?;
    let read = |path: PathBuf, pointer: &str| {
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let value = serde_json::from_str::<Value>(&text)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        value
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("{}: no {pointer}", path.display()))
    };
    Ok(Curl {
        base: BASE.to_owned(),
        token: read(
            home.join(".claude/.credentials.json"),
            "/claudeAiOauth/accessToken",
        )?,
        org: read(home.join(".claude.json"), "/oauthAccount/organizationUuid")?,
    })
}

pub(super) fn execute(command: Cloud, api: &mut Api, out: &mut dyn Write) -> Result<u8, String> {
    match command {
        Cloud::List => {
            for session in pages(api, "/v1/sessions?beta=true&limit=100")? {
                let _ = writeln!(out, "{}", row(&session));
            }
        }
        Cloud::Events(id, path) => {
            let events = events(api, &id)?;
            save(&events, &path)?;
            let _ = writeln!(out, "{} events in {}", events.len(), path.display());
        }
        Cloud::Sweep(dir) => sweep(api, &dir, out)?,
        Cloud::Launch(..) => return Err("launch starts a session through claude --cloud, not the sessions API".into()),
        Cloud::Collect(..) => return Err("collect reads a saved transcript, not the sessions API".into()),
        Cloud::Send(id, file) => {
            let text = std::fs::read_to_string(&file)
                .map_err(|error| format!("{}: {error}", file.display()))?;
            let uuid = crate::fleet::runtime::new_uuid().map_err(|error| error.user_message())?;
            let status = send(api, &id, &uuid, &text)?;
            let _ = writeln!(out, "{status} {uuid}");
            return Ok(if (200..300).contains(&status) { 0 } else { 1 });
        }
        Cloud::Wait(id, path, timeout, after) => {
            let polls = timeout / POLL.as_secs();
            let Some(count) = wait(api, &id, &path, after.as_deref(), polls, &mut || {
                std::thread::sleep(POLL)
            })?
            else {
                let _ = writeln!(out, "{id} did not finish its turn within {timeout} s");
                return Ok(1);
            };
            let _ = writeln!(
                out,
                "{id} finished its turn; {count} events in {}",
                path.display()
            );
        }
    }
    Ok(0)
}

pub fn run(arguments: &[String]) -> u8 {
    let Some(command) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 2;
    };
    if let Cloud::Launch(repo, slug, brief, item) = &command {
        return match crate::ops::dispatch::cloud_launch(repo, slug, brief, item.as_deref()) {
            Ok(line) => {
                println!("{line}");
                0
            }
            Err(error) => {
                eprintln!("north cloud launch: {error}");
                1
            }
        };
    }
    if let Cloud::Collect(events, out) = &command {
        return match super::cloud_handback::run(events, out, &mut std::io::stdout()) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("north cloud collect: {error}");
                1
            }
        };
    }
    let result = credentials().and_then(|curl| {
        execute(
            command,
            &mut |method, path, body| curl.call(method, path, body),
            &mut std::io::stdout(),
        )
    });
    result.unwrap_or_else(|error| {
        eprintln!("north cloud: {error}");
        1
    })
}

#[cfg(test)]
#[path = "cloud_tests.rs"]
mod tests;
