use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::codex::chat_updates;
use crate::error::{NorthError, NorthResult};
use crate::fleet::gates::Question;
use crate::fleet::model::{Account, Entry, EntryKind, LaunchRequest, ProviderName, Quota, QuotaWindow, Status, Transcript};
use crate::fleet::roster::Roster;
use crate::fleet::turns::{Tokens, TurnExit, TurnSignal, TurnTotals};
use crate::fleet::runtime::{HostSpec, Instructions, Opening, Runtime, SESSION_ENV, new_uuid, summarize};

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
            env_remove: Vec::new(),
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
        let mut params = json!({"approvalPolicy": "never", "sandbox": "danger-full-access", "config": {format!("shell_environment_policy.set.{SESSION_ENV}"): name}});
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
    use crate::fleet::prompt::PromptMode;
    use crate::fleet::runtime::Instructions;
    use crate::fleet::turns::Turns;

    use super::*;

    fn request() -> LaunchRequest {
        LaunchRequest {
            verification: false,
            name: "t".into(),
            provider: Codex.provider(),
            account: "pooled".into(),
            cwd: PathBuf::from("/tmp"),
            model: None,
            prompt: String::new(),
            system_prompt: None,
            foreground_cap_secs: None,
            prompt_mode: PromptMode::North,
            idle_exit_minutes: None,
            parent: None,
            task_ref: None,
            placement: None,
            machine: None,
            capacity_need: None,
        }
    }

    #[test]
    fn threads_open_with_full_access_like_claude_sessions() {
        for opening in [Opening::Fresh(None), Opening::Resume("th")] {
            let frame: Value = serde_json::from_str(&Codex.open("t", &request(), opening, Instructions { text: "", changed: true }).unwrap()[0]).unwrap();
            assert_eq!(frame["params"]["sandbox"], "danger-full-access", "{opening:?}");
        }
    }

    #[test]
    fn every_thread_names_its_session_to_the_commands_it_runs() {
        for opening in [Opening::Fresh(None), Opening::Resume("th")] {
            let frame: Value = serde_json::from_str(&Codex.open("worker", &request(), opening, Instructions { text: "", changed: false }).unwrap()[0]).unwrap();
            assert_eq!(frame["params"]["config"]["shell_environment_policy.set.NORTH_FLEET_SESSION"], "worker", "{opening:?}");
        }
    }

    #[test]
    fn north_instructions_join_codex_own_and_a_changed_rendering_reaches_a_loaded_thread() {
        let open = |opening, changed| -> Vec<Value> {
            let frames = Codex.open("t", &request(), opening, Instructions { text: "standing orders", changed }).unwrap();
            frames.iter().map(|frame| serde_json::from_str(frame).unwrap()).collect()
        };
        for opening in [Opening::Fresh(None), Opening::Resume("th")] {
            let frame = &open(opening, true)[0];
            assert_eq!(frame["params"]["developerInstructions"], "standing orders", "{opening:?}");
            assert!(frame["params"]["baseInstructions"].is_null(), "{opening:?} replaced Codex's own instructions");
        }
        let refreshed = open(Opening::Resume("th"), true);
        let injected = &refreshed.last().unwrap()["params"];
        assert_eq!((injected["threadId"].as_str(), injected["items"][0]["role"].as_str()), (Some("th"), Some("developer")));
        assert_eq!(injected["items"][0]["content"][0]["text"], "standing orders");
        assert_eq!(open(Opening::Resume("th"), false).len(), 1, "an unchanged rendering must not grow the thread");
    }

    #[test]
    fn a_window_that_opens_mid_turn_still_sees_the_turn_and_can_interrupt_it() {
        let mut transcript = Transcript::new();
        transcript.name = "t".into();
        transcript.conversation = Some("th".into());
        let delta = json!({"method": "item/agentMessage/delta", "params": {"threadId": "th", "turnId": "t9", "itemId": "m", "delta": "16"}});
        Codex.observe(&delta, &mut transcript);
        assert_eq!((transcript.status, transcript.active_turn.as_deref()), (Status::Working, Some("t9")));
        let interrupt: Value = serde_json::from_str(&Codex.interrupt(&transcript).unwrap()[0]).unwrap();
        assert_eq!((interrupt["method"].as_str(), interrupt["params"]["turnId"].as_str()), (Some("turn/interrupt"), Some("t9")));
        let usage = json!({"method": "thread/tokenUsage/updated", "params": {"threadId": "th", "turnId": "t9", "tokenUsage": {"total": {}}}});
        let completed = json!({"method": "turn/completed", "params": {"threadId": "th", "turn": {"id": "t9", "status": "completed"}}});
        Codex.observe(&completed, &mut transcript);
        Codex.observe(&usage, &mut transcript);
        assert_eq!(transcript.status, Status::Idle, "usage reported after the turn must not reopen it");
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

#[cfg(test)]
mod quota {
    use super::*;

    #[test]
    fn a_rate_limits_update_reports_each_window_used() {
        let frame = json!({"method": "account/rateLimits/updated", "params": {"rateLimits": {"limitId": "codex",
            "primary": {"usedPercent": 22, "windowDurationMins": 10080, "resetsAt": 1792649600},
            "secondary": {"usedPercent": 70, "windowDurationMins": 300}}}});
        let quota = Codex.quota(&frame).unwrap();
        let windows: Vec<(&str, u64)> = quota.windows.iter().map(|window| (window.label.as_str(), window.left_percent())).collect();
        assert_eq!(windows, [("7d", 78), ("5h", 30)]);
        assert_eq!(Codex.quota(&json!({"method": "turn/started"})), None);
    }
}
