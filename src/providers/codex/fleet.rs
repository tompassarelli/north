use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::codex::chat_updates;
use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Account, Entry, EntryKind, LaunchRequest, ProviderName, Status, Transcript};
use crate::fleet::runtime::{HostSpec, Opening, Runtime, binary, new_uuid, summarize};

use super::accounts;

const PROVIDER: &str = "codex";
const COMMAND_CHARS: usize = 400;

pub struct Codex;

impl Runtime for Codex {
    fn provider(&self) -> ProviderName {
        ProviderName::new(PROVIDER)
    }

    fn accounts(&self, state_root: &Path, home: &Path) -> Vec<Account> {
        accounts::discover(&self.provider(), state_root, home)
    }

    fn assign_conversation(&self) -> NorthResult<Option<String>> {
        Ok(None)
    }

    fn host(&self, account: &Account, _request: &LaunchRequest, _opening: Opening) -> HostSpec {
        let mut argv = vec![binary("codex").display().to_string()];
        argv.extend(["app-server", "--listen", "stdio://", "--enable", "multi_agent_v2"].map(str::to_owned));
        HostSpec {
            key: format!("codex-{}", account.label),
            shared: true,
            argv,
            env: codex_env(&account.home),
            cwd: std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from),
        }
    }

    fn handshake(&self) -> Vec<String> {
        let initialize = json!({
            "id": format!("fleet:initialize:{}", new_uuid().unwrap_or_default()),
            "method": "initialize",
            "params": {
                "clientInfo": {"name": "north", "title": "North fleet", "version": env!("CARGO_PKG_VERSION")},
                "capabilities": {"experimentalApi": true}
            }
        });
        vec![initialize.to_string(), json!({"method": "initialized", "params": {}}).to_string()]
    }

    fn open(&self, name: &str, request: &LaunchRequest, opening: Opening) -> NorthResult<Vec<String>> {
        let mut params = json!({"approvalPolicy": "never", "sandbox": "workspace-write"});
        if let Some(model) = &request.model {
            params["model"] = json!(model);
        }
        let method = match opening {
            Opening::Resume(thread) => {
                params["threadId"] = json!(thread);
                "thread/resume"
            }
            Opening::Fresh(_) => {
                params["cwd"] = json!(request.cwd);
                if let Some(path) = &request.system_prompt {
                    params["baseInstructions"] = json!(std::fs::read_to_string(path)?);
                }
                "thread/start"
            }
        };
        Ok(vec![json!({"id": open_id(name), "method": method, "params": params}).to_string()])
    }

    fn message(&self, transcript: &Transcript, text: &str) -> NorthResult<Vec<String>> {
        let thread = thread(transcript)?;
        let input = json!([{"type": "text", "text": text}]);
        let frame = match &transcript.active_turn {
            Some(turn) => request(transcript, "turn/steer", json!({"threadId": thread, "expectedTurnId": turn, "input": input}))?,
            None => request(transcript, "turn/start", json!({"threadId": thread, "input": input}))?,
        };
        Ok(vec![frame])
    }

    fn interrupt(&self, transcript: &Transcript) -> NorthResult<Vec<String>> {
        let thread = thread(transcript)?;
        let Some(turn) = &transcript.active_turn else { return Ok(Vec::new()) };
        Ok(vec![request(transcript, "turn/interrupt", json!({"threadId": thread, "turnId": turn}))?])
    }

    fn close(&self, transcript: &Transcript) -> Vec<String> {
        let Ok(thread) = thread(transcript) else { return Vec::new() };
        let mut frames = self.interrupt(transcript).unwrap_or_default();
        frames.extend(request(transcript, "thread/unsubscribe", json!({"threadId": thread})));
        frames
    }

    fn observe(&self, frame: &Value, transcript: &mut Transcript) {
        if let Some(id) = frame["id"].as_str() {
            observe_response(id, frame, transcript);
            return;
        }
        let params = &frame["params"];
        let thread = params["threadId"].as_str().or_else(|| params["thread"]["id"].as_str());
        if thread.is_none() || thread != transcript.conversation.as_deref() {
            return;
        }
        match frame["method"].as_str() {
            Some("turn/started") => {
                transcript.status = Status::Working;
                transcript.active_turn = params["turn"]["id"].as_str().map(str::to_owned);
            }
            Some("turn/completed") => observe_turn_completed(params, transcript),
            Some("item/completed") => observe_item(frame, transcript),
            Some("error") => observe_error(params, transcript),
            _ => {}
        }
    }
}

fn codex_env(home: &Path) -> Vec<(String, OsString)> {
    let mut env = vec![("CODEX_HOME".into(), home.as_os_str().to_owned())];
    let sqlite = home.join("sqlite");
    if sqlite.is_dir() {
        env.push(("CODEX_SQLITE_HOME".into(), sqlite.into_os_string()));
    }
    env
}

fn open_id(name: &str) -> String {
    format!("{name}:thread-open")
}

fn thread(transcript: &Transcript) -> NorthResult<&str> {
    transcript
        .conversation
        .as_deref()
        .ok_or_else(|| NorthError::State(format!("{} has no Codex thread yet", transcript.name)))
}

fn request(transcript: &Transcript, method: &str, params: Value) -> NorthResult<String> {
    let id = format!("{}:{method}:{}", transcript.name, new_uuid()?);
    Ok(json!({"id": id, "method": method, "params": params}).to_string())
}

fn observe_response(id: &str, frame: &Value, transcript: &mut Transcript) {
    let ours = id.strip_prefix(transcript.name.as_str()).is_some_and(|rest| rest.starts_with(':'));
    if !ours {
        return;
    }
    if id == open_id(&transcript.name) {
        if let Some(thread) = frame["result"]["thread"]["id"].as_str() {
            transcript.conversation = Some(thread.to_owned());
            transcript.status = Status::Idle;
        }
    }
    if let Some(message) = frame["error"]["message"].as_str() {
        push(transcript, EntryKind::Error, message.to_owned());
    }
}

fn observe_error(params: &Value, transcript: &mut Transcript) {
    let kind = if params["willRetry"] == true { EntryKind::Notice } else { EntryKind::Error };
    push(transcript, kind, params["error"]["message"].as_str().unwrap_or("error").to_owned());
}

fn observe_turn_completed(params: &Value, transcript: &mut Transcript) {
    transcript.status = Status::Idle;
    transcript.active_turn = None;
    if let Some(message) = params["turn"]["error"]["message"].as_str() {
        push(transcript, EntryKind::Error, message.to_owned());
    }
    push(transcript, EntryKind::TurnEnd, params["turn"]["status"].as_str().unwrap_or("completed").to_owned());
}

fn observe_item(frame: &Value, transcript: &mut Transcript) {
    for update in chat_updates(frame) {
        let (kind, text) = match update.kind.as_str() {
            "userMessage" => (EntryKind::User, update.text),
            "agentMessage" => (EntryKind::Assistant, update.text),
            "commandExecution" | "fileChange" => (EntryKind::Tool, summarize(&update.text, COMMAND_CHARS)),
            _ => continue,
        };
        push(transcript, kind, text);
    }
}

fn push(transcript: &mut Transcript, kind: EntryKind, text: String) {
    if !text.is_empty() {
        transcript.entries.push(Entry { kind, text });
    }
}

#[cfg(test)]
mod projection {
    use super::*;

    fn project(name: &str) -> Transcript {
        let mut transcript = Transcript::new();
        transcript.name = name.into();
        for line in include_str!("fixtures/fleet.jsonl").lines() {
            Codex.observe(&serde_json::from_str(line).unwrap(), &mut transcript);
        }
        transcript
    }

    #[test]
    fn codex_turn_projects_only_its_own_thread() {
        let transcript = project("t-codex");
        let kinds: Vec<(EntryKind, &str)> = transcript.entries.iter().map(|entry| (entry.kind, entry.text.as_str())).collect();
        assert_eq!(kinds, [(EntryKind::User, "Reply with exactly: pong"), (EntryKind::Assistant, "pong"), (EntryKind::TurnEnd, "completed")]);
        assert_eq!(transcript.status, Status::Idle);
        assert_eq!(transcript.active_turn, None);
        let stranger = project("other");
        assert!(stranger.entries.is_empty());
        assert_eq!(stranger.conversation, None);
    }
}
