use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::codex::chat_updates;
use crate::error::{NorthError, NorthResult};
use crate::fleet::attachments;
use crate::fleet::gates::Question;
use crate::fleet::model::{Account, Entry, EntryKind, LaunchRequest, ProviderName, Quota, QuotaWindow, Status, Transcript};
use crate::fleet::roster::Roster;
use crate::fleet::turns::{Tokens, TurnExit, TurnSignal, TurnTotals};
use crate::fleet::runtime::{HostSpec, ACTOR_ENV, Instructions, Opening, Runtime, ROLE_ENV, SESSION_ENV, new_uuid, summarize};

use super::{accounts, launch, questions};

const PROVIDER: &str = "codex";
const COMMAND_CHARS: usize = 400;
const MODELS: &[&str] = &["gpt-6-astra", "gpt-6.1-sol", "gpt-6-sol", "gpt-6-luna", "gpt-5.6-sol", "gpt-5.6-terra", "gpt-5.6-luna"];

pub struct Codex;

impl Runtime for Codex {
    fn provider(&self) -> ProviderName {
        ProviderName::new(PROVIDER)
    }

    fn models(&self) -> &'static [&'static str] {
        MODELS
    }

    fn roster(&self, state_root: &Path, home: &Path, _policy: &Path) -> Roster {
        accounts::discover(&self.provider(), state_root, home)
    }

    fn assign_conversation(&self) -> NorthResult<Option<String>> {
        Ok(None)
    }

    fn host(&self, account: &Account, request: &LaunchRequest, _opening: Opening, _instructions: &str) -> NorthResult<HostSpec> {
        let server = launch::session_server(&account.home)?;
        Ok(HostSpec {
            key: request.name.clone(),
            shared: false,
            argv: server.argv,
            env: server.env,
            env_remove: vec![ROLE_ENV.into(), SESSION_ENV.into()],
            cwd: std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from),
        })
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

    fn open(&self, name: &str, request: &LaunchRequest, opening: Opening, instructions: Instructions) -> NorthResult<Vec<String>> {
        let mut params = json!({"approvalPolicy": "never", "sandbox": "danger-full-access", "config": {format!("shell_environment_policy.set.{SESSION_ENV}"): name, format!("shell_environment_policy.set.{ACTOR_ENV}"): name, format!("shell_environment_policy.set.{ROLE_ENV}"): request.role().label()}});
        if let Some(model) = &request.model {
            params["model"] = json!(model);
        }
        if !instructions.text.is_empty() {
            params["developerInstructions"] = json!(instructions.text);
        }
        let (method, refresh) = match opening {
            Opening::Resume(thread) => {
                params["threadId"] = json!(thread);
                let refresh = (instructions.changed && !instructions.text.is_empty()).then(|| developer_message(name, thread, instructions.text));
                ("thread/resume", refresh.transpose()?)
            }
            Opening::Fresh(_) => {
                params["cwd"] = json!(request.cwd);
                ("thread/start", None)
            }
        };
        let open = json!({"id": open_id(name), "method": method, "params": params}).to_string();
        Ok([Some(open), refresh].into_iter().flatten().collect())
    }

    fn message(&self, transcript: &Transcript, _id: &str, text: &str) -> NorthResult<Vec<String>> {
        if !attachments::paths(text).is_empty() {
            return Err(NorthError::Usage("codex sessions do not take image attachments".into()));
        }
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
        if let Some(turn) = turn_in_progress(frame) {
            transcript.status = Status::Working;
            transcript.active_turn = Some(turn.to_owned());
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

    fn frame_conversation<'a>(&self, frame: &'a Value) -> Option<&'a str> {
        frame["params"]["threadId"].as_str().or(frame["params"]["thread"]["id"].as_str()).or(frame["result"]["thread"]["id"].as_str())
    }

    fn quota(&self, frame: &Value) -> Option<Quota> {
        account_quota(frame)
    }

    fn question(&self, frame: &Value) -> Option<Question> {
        questions::question(frame)
    }
}

fn turn_in_progress(frame: &Value) -> Option<&str> {
    frame["method"].as_str().filter(|method| method.starts_with("item/")).and(frame["params"]["turnId"].as_str())
}

fn developer_message(name: &str, thread: &str, text: &str) -> NorthResult<String> {
    let item = json!({"type": "message", "role": "developer", "content": [{"type": "input_text", "text": text}]});
    let id = format!("{name}:thread/inject_items:{}", new_uuid()?);
    Ok(json!({"id": id, "method": "thread/inject_items", "params": {"threadId": thread, "items": [item]}}).to_string())
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
    if id == open_id(&transcript.name)
        && let Some(thread) = frame["result"]["thread"]["id"].as_str() {
            transcript.conversation = Some(thread.to_owned());
            transcript.status = Status::Idle;
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

fn account_quota(frame: &Value) -> Option<Quota> {
    if frame["method"] != "account/rateLimits/updated" {
        return None;
    }
    let limits = &frame["params"]["rateLimits"];
    let windows = [&limits["primary"], &limits["secondary"]]
        .into_iter()
        .filter_map(|window| Some(QuotaWindow::spanning(window["windowDurationMins"].as_u64()?, window["usedPercent"].as_u64()?)))
        .collect();
    Some(Quota { windows })
}
