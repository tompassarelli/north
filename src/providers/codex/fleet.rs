use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::codex::chat_updates;
use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Account, Entry, EntryKind, LaunchRequest, ProviderName, Status, Transcript};
use crate::fleet::turns::{Tokens, TurnExit, TurnSignal, TurnTotals};
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
        let mut params = json!({"approvalPolicy": "never", "sandbox": "danger-full-access"});
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

    fn signals(&self, frame: &Value) -> Vec<TurnSignal> {
        turn_signals(frame)
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

fn turn_signals(frame: &Value) -> Vec<TurnSignal> {
    if frame["id"].as_str().is_some_and(|id| id.ends_with(":thread-open")) {
        return opened(&frame["result"]).into_iter().collect();
    }
    let params = &frame["params"];
    let Some(conversation) = params["threadId"].as_str().map(str::to_owned) else { return Vec::new() };
    let signal = match frame["method"].as_str() {
        Some("turn/started") => TurnSignal::Started { conversation },
        Some("item/completed") if params["item"]["type"] == "userMessage" => TurnSignal::Prompt { conversation, text: message_text(&params["item"]) },
        Some("thread/tokenUsage/updated") => TurnSignal::Usage { conversation, total: usage_tokens(&params["tokenUsage"]["total"]) },
        Some("turn/completed") => TurnSignal::Ended {
            conversation,
            exit: turn_exit(params["turn"]["status"].as_str()),
            totals: TurnTotals { wall_ms: params["turn"]["durationMs"].as_u64(), ..TurnTotals::default() },
        },
        _ => return Vec::new(),
    };
    vec![signal]
}

fn opened(result: &Value) -> Option<TurnSignal> {
    Some(TurnSignal::Opened {
        conversation: result["thread"]["id"].as_str()?.to_owned(),
        model: result["model"].as_str().map(str::to_owned),
        effort: result["reasoningEffort"].as_str().map(str::to_owned),
    })
}

fn message_text(item: &Value) -> String {
    item["content"].as_array().into_iter().flatten().filter_map(|part| part["text"].as_str()).collect::<Vec<_>>().join("\n")
}

fn usage_tokens(total: &Value) -> Tokens {
    let count = |key: &str| total[key].as_u64().unwrap_or(0);
    Tokens {
        input: count("inputTokens").saturating_sub(count("cachedInputTokens")),
        output: count("outputTokens"),
        cache_read: count("cachedInputTokens"),
        cache_write: count("cacheWriteInputTokens"),
    }
}

fn turn_exit(status: Option<&str>) -> TurnExit {
    match status {
        Some("interrupted") => TurnExit::Interrupted,
        Some("failed") => TurnExit::Failed,
        _ => TurnExit::Completed,
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

#[cfg(test)]
mod turn_state {
    use crate::fleet::turns::Turns;

    use super::*;

    fn request() -> LaunchRequest {
        LaunchRequest {
            name: "t".into(),
            provider: Codex.provider(),
            account: "pooled".into(),
            cwd: PathBuf::from("/tmp"),
            model: None,
            prompt: String::new(),
            system_prompt: None,
            foreground_cap_secs: None,
            idle_exit_minutes: None,
            parent: None,
            task_ref: None,
        }
    }

    #[test]
    fn threads_open_with_full_access_like_claude_sessions() {
        for opening in [Opening::Fresh(None), Opening::Resume("th")] {
            let frame: Value = serde_json::from_str(&Codex.open("t", &request(), opening).unwrap()[0]).unwrap();
            assert_eq!(frame["params"]["sandbox"], "danger-full-access", "{opening:?}");
        }
    }

    #[test]
    fn cumulative_thread_usage_becomes_per_turn_tokens() {
        let usage = |input: u64, cached: u64, output: u64| {
            json!({"method": "thread/tokenUsage/updated", "params": {"threadId": "th", "tokenUsage": {"total": {"inputTokens": input, "cachedInputTokens": cached, "cacheWriteInputTokens": 0, "outputTokens": output}}}})
        };
        let started = json!({"method": "turn/started", "params": {"threadId": "th", "turn": {"id": "t2"}}});
        let completed = json!({"method": "turn/completed", "params": {"threadId": "th", "turn": {"id": "t2", "status": "completed", "durationMs": 1822}}});
        let mut turns = Turns::default();
        let mut finished = Vec::new();
        for frame in [usage(17156, 11008, 5), started, usage(38350, 27136, 10), completed] {
            finished.extend(Codex.signals(&frame).into_iter().filter_map(|signal| turns.observe(signal, 0)));
        }
        let turn = &finished;
        assert_eq!(turn.len(), 1);
        assert_eq!((turn[0].tokens, turn[0].turns, turn[0].wall_ms), (Tokens { input: 5066, output: 5, cache_read: 16128, cache_write: 0 }, 1, 1822));
    }
}
