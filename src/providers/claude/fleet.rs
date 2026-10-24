use std::path::Path;

use serde_json::Value;

use crate::claude_stream::{encode_interrupt, encode_user_message};
use crate::error::NorthResult;
use crate::fleet::model::{Account, Entry, EntryKind, LaunchRequest, ProviderName, Quota, Status, Transcript};
use crate::fleet::roster::Roster;
use crate::fleet::turns::TurnSignal;
use crate::fleet::runtime::{CredentialFailure, HostSpec, Instructions, Opening, Runtime, new_uuid, summarize};
use crate::providers::install;

use super::session_env::{self, claude_env};
use super::{accounts, quota, turns, user_text};

const PROVIDER: &str = "claude";
const TOOL_RESULT_CHARS: usize = 400;
const MODELS: &[&str] = &["fable", "opus", "sonnet", "haiku", "claude-fable-5-1", "claude-opus-5-5", "claude-sonnet-5-5", "claude-haiku-5-5"];

pub struct Claude;

impl Runtime for Claude {
    fn provider(&self) -> ProviderName {
        ProviderName::new(PROVIDER)
    }

    fn models(&self) -> &'static [&'static str] {
        MODELS
    }

    fn roster(&self, state_root: &Path, home: &Path) -> Roster {
        accounts::discover(&self.provider(), state_root, &accounts::declared(home))
    }

    fn assign_conversation(&self) -> NorthResult<Option<String>> {
        new_uuid().map(Some)
    }

    fn host(&self, account: &Account, request: &LaunchRequest, opening: Opening, instructions: &str) -> NorthResult<HostSpec> {
        let mut argv = vec![install::runtime("claude").display().to_string()];
        argv.extend(
            [
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--replay-user-messages",
                "--strict-mcp-config",
                "--permission-mode",
                "bypassPermissions",
                "--system-prompt-snapshot",
                "off",
            ]
            .map(str::to_owned),
        );
        match opening {
            Opening::Fresh(Some(conversation)) => argv.extend(["--session-id".into(), conversation.into()]),
            Opening::Resume(conversation) => argv.extend(["--resume".into(), conversation.into()]),
            Opening::Fresh(None) => {}
        }
        if let Some(model) = &request.model {
            argv.extend(["--model".into(), model.clone()]);
        }
        if !instructions.is_empty() {
            argv.extend(["--append-system-prompt".into(), instructions.to_owned()]);
        }
        Ok(HostSpec {
            key: request.name.clone(),
            shared: false,
            argv,
            env: claude_env(account, request),
            cwd: request.cwd.clone(),
        })
    }

    fn handshake(&self) -> Vec<String> {
        Vec::new()
    }

    fn open(&self, _name: &str, _request: &LaunchRequest, _opening: Opening, _instructions: Instructions) -> NorthResult<Vec<String>> {
        Ok(Vec::new())
    }

    fn message(&self, _transcript: &Transcript, text: &str) -> NorthResult<Vec<String>> {
        Ok(vec![encode_user_message(&new_uuid()?, text)])
    }

    fn interrupt(&self, _transcript: &Transcript) -> NorthResult<Vec<String>> {
        Ok(vec![encode_interrupt(&format!("interrupt-{}", new_uuid()?))])
    }

    fn close(&self, _transcript: &Transcript) -> Vec<String> {
        Vec::new()
    }

    fn observe(&self, frame: &Value, transcript: &mut Transcript) {
        if transcript.status == Status::Starting {
            transcript.status = Status::Idle;
        }
        if resumes_after_result(frame, transcript) {
            transcript.status = Status::Working;
        }
        match frame["type"].as_str() {
            Some("system") => observe_system(frame, transcript),
            Some("user") => observe_user(frame, transcript),
            Some("assistant") => observe_assistant(frame, transcript),
            Some("result") => observe_result(frame, transcript),
            _ => {}
        }
    }

    fn signals(&self, frame: &Value) -> Vec<TurnSignal> {
        turns::signals(frame)
    }

    fn frame_conversation<'a>(&self, frame: &'a Value) -> Option<&'a str> {
        frame["session_id"].as_str()
    }

    fn quota(&self, frame: &Value) -> Option<Quota> {
        quota::account_quota(frame)
    }

    fn credential_failure(&self, frame: &Value) -> Option<CredentialFailure> {
        turns::credential_failure(frame)
    }

    fn caller_conversation(&self) -> Option<String> {
        session_env::caller_conversation()
    }
}

fn observe_system(frame: &Value, transcript: &mut Transcript) {
    if frame["subtype"] == "init"
        && let Some(session) = frame["session_id"].as_str() {
            transcript.conversation = Some(session.to_owned());
        }
}

fn observe_user(frame: &Value, transcript: &mut Transcript) {
    let content = &frame["message"]["content"];
    if let Some(text) = content.as_str() {
        observe_user_text(frame, text, transcript);
        return;
    }
    for block in content.as_array().into_iter().flatten() {
        match block["type"].as_str() {
            Some("tool_result") => push(transcript, EntryKind::ToolResult, summarize(&tool_result_text(block), TOOL_RESULT_CHARS)),
            Some("text") => observe_user_text(frame, block["text"].as_str().unwrap_or_default(), transcript),
            _ => {}
        }
    }
}

fn observe_user_text(frame: &Value, text: &str, transcript: &mut Transcript) {
    transcript.status = Status::Working;
    transcript.active_turn = frame["uuid"].as_str().map(str::to_owned);
    let (kind, text) = user_text::entry(text, frame["isReplay"] == true, frame["isSynthetic"] == true);
    push(transcript, kind, text);
}

fn tool_result_text(block: &Value) -> String {
    match &block["content"] {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts.iter().filter_map(|part| part["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

fn observe_assistant(frame: &Value, transcript: &mut Transcript) {
    for block in frame["message"]["content"].as_array().into_iter().flatten() {
        match block["type"].as_str() {
            Some("text") => push(transcript, EntryKind::Assistant, block["text"].as_str().unwrap_or_default().to_owned()),
            Some("tool_use") => push(transcript, EntryKind::Tool, tool_use_text(block)),
            _ => {}
        }
    }
}

fn tool_use_text(block: &Value) -> String {
    let name = block["name"].as_str().unwrap_or("tool");
    let input = &block["input"];
    let detail = ["command", "file_path", "pattern", "description", "prompt"]
        .iter()
        .find_map(|key| input[*key].as_str())
        .map_or_else(|| input.to_string(), str::to_owned);
    format!("{name} {}", summarize(&detail, TOOL_RESULT_CHARS))
}

fn observe_result(frame: &Value, transcript: &mut Transcript) {
    transcript.status = Status::Idle;
    transcript.active_turn = None;
    let subtype = frame["subtype"].as_str().unwrap_or("result");
    let aborted = frame["terminal_reason"].as_str().is_some_and(|reason| reason.starts_with("aborted"));
    if frame["is_error"] == true && !aborted {
        let detail = frame["result"].as_str().unwrap_or(subtype);
        push(transcript, EntryKind::Error, detail.to_owned());
    }
    let label = if aborted { "interrupted".to_owned() } else { subtype.to_owned() };
    push(transcript, EntryKind::TurnEnd, label);
}

fn resumes_after_result(frame: &Value, transcript: &Transcript) -> bool {
    let ended = transcript.status == Status::Idle && transcript.entries.last().is_some_and(|entry| entry.kind == EntryKind::TurnEnd);
    ended && frame["type"] == "assistant" && frame["parent_tool_use_id"].is_null()
}

fn push(transcript: &mut Transcript, kind: EntryKind, text: String) {
    if !text.is_empty() {
        transcript.entries.push(Entry { kind, text });
    }
}
#[cfg(test)]
mod projection {
    use super::*;

    fn projected(fixture: &str) -> (Transcript, Vec<(EntryKind, String)>) {
        let mut transcript = Transcript::new();
        fixture.lines().for_each(|line| Claude.observe(&serde_json::from_str(line).unwrap(), &mut transcript));
        let kinds = transcript.entries.iter().map(|entry| (entry.kind, entry.text.clone())).collect();
        (transcript, kinds)
    }

    #[test]
    fn claude_turn_projects_to_user_assistant_and_idle() {
        let (transcript, kinds) = projected(include_str!("fixtures/fleet.jsonl"));
        assert_eq!(kinds, [(EntryKind::User, "Reply with exactly: pong".into()), (EntryKind::Assistant, "pong".into()), (EntryKind::TurnEnd, "success".into())]);
        assert_eq!(transcript.status, Status::Idle);
        assert_eq!(transcript.conversation.as_deref(), Some("7557fc31-3bae-471d-8307-ce4fc827601c"));
    }

    #[test]
    fn a_local_command_projects_to_the_operator_command_and_its_outcome() {
        let (transcript, kinds) = projected(include_str!("fixtures/compact.jsonl"));
        assert_eq!(kinds, [
            (EntryKind::Notice, "This session is being continued from a previous conversation that ran out of context.".into()),
            (EntryKind::Notice, "Compacted".into()),
            (EntryKind::User, "/compact".into()),
            (EntryKind::TurnEnd, "success".into()),
        ]);
        assert_eq!(transcript.status, Status::Idle);
    }
}

#[cfg(test)]
mod turn_state {
    use serde_json::json;

    use super::*;
    use crate::fleet::turns::{Tokens, TurnExit, TurnTotals};

    #[test]
    fn command_shaped_message_text_reaches_claude_verbatim() {
        let text = "please run north gate open --session x --question q";
        let frame = Claude.message(&Transcript::new(), text).unwrap();
        let frame: Value = serde_json::from_str(&frame[0]).unwrap();
        assert_eq!(frame["message"]["content"].as_str(), Some(text));
    }

    #[test]
    fn frames_after_a_result_mean_the_session_works_again() {
        let mut transcript = Transcript::new();
        let finished = [
            json!({"type": "user", "isReplay": true, "session_id": "s", "uuid": "u1", "message": {"content": "go"}}),
            json!({"type": "result", "subtype": "success", "session_id": "s", "is_error": false}),
            json!({"type": "command_lifecycle", "state": "completed", "session_id": "s"}),
        ];
        for frame in &finished {
            Claude.observe(frame, &mut transcript);
        }
        assert_eq!(transcript.status, Status::Idle);
        let resumed = json!({"type": "assistant", "session_id": "s", "parent_tool_use_id": null, "message": {"content": [{"type": "text", "text": "the task reported back"}]}});
        Claude.observe(&resumed, &mut transcript);
        assert_eq!(transcript.status, Status::Working);
    }

    #[test]
    fn revive_hook_frames_leave_an_idle_session_idle() {
        let mut transcript = Transcript::new();
        for frame in [
            json!({"type": "result", "subtype": "success", "session_id": "s", "is_error": false}),
            json!({"type": "command_lifecycle", "state": "started", "session_id": "s"}),
            json!({"type": "rate_limit_event", "session_id": "s"}),
        ] {
            Claude.observe(&frame, &mut transcript);
        }
        assert_eq!(transcript.status, Status::Idle);
    }

    #[test]
    fn a_new_claude_user_message_opens_a_turn_before_assistant_output() {
        let user = json!({"type": "user", "session_id": "s", "uuid": "u", "message": {"content": "continue the task"}});
        let mut transcript = Transcript::new();
        Claude.observe(&user, &mut transcript);
        assert_eq!(transcript.status, Status::Working);
        assert!(matches!(Claude.signals(&user).as_slice(), [TurnSignal::Started { .. }, TurnSignal::Prompt { .. }]));
    }

    #[test]
    fn results_carry_turn_usage_and_tasks_announce_themselves() {
        let result = json!({
            "type": "result", "session_id": "s", "is_error": false, "num_turns": 3, "duration_ms": 1200,
            "usage": {"input_tokens": 7, "output_tokens": 40, "cache_read_input_tokens": 900, "cache_creation_input_tokens": 300}
        });
        let tokens = Tokens { input: 7, output: 40, cache_read: 900, cache_write: 300 };
        let totals = TurnTotals { tokens: Some(tokens), turns: Some(3), wall_ms: Some(1200) };
        assert_eq!(Claude.signals(&result), [TurnSignal::Ended { conversation: "s".into(), exit: TurnExit::Completed, totals }]);
        let started = json!({"type": "system", "subtype": "task_started", "session_id": "s", "task_id": "a1"});
        let ended = json!({"type": "system", "subtype": "task_notification", "session_id": "s", "task_id": "a1", "status": "completed"});
        let subagent = json!({"type": "assistant", "session_id": "s", "parent_tool_use_id": "toolu_1", "message": {"content": []}});
        assert_eq!(Claude.signals(&started), [TurnSignal::TaskStarted { conversation: "s".into(), task: "a1".into() }]);
        assert_eq!(Claude.signals(&ended), [TurnSignal::TaskEnded { conversation: "s".into(), task: "a1".into() }]);
        assert_eq!(Claude.signals(&subagent), []);
    }

    #[test]
    fn an_assistant_frame_reports_its_call_usage_while_the_turn_runs() {
        let frame = json!({
            "type": "assistant", "session_id": "s", "parent_tool_use_id": null,
            "message": {"id": "msg_1", "content": [], "usage": {"input_tokens": 5, "output_tokens": 9, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 2}}
        });
        let tokens = Tokens { input: 5, output: 9, cache_read: 100, cache_write: 2 };
        assert_eq!(Claude.signals(&frame), [TurnSignal::Started { conversation: "s".into() }, TurnSignal::Call { conversation: "s".into(), call: "msg_1".into(), tokens }]);
    }
}
#[cfg(test)]
mod launch {
    use std::path::PathBuf;

    use crate::fleet::prompt::PromptMode;

    use super::*;

    #[test]
    fn the_host_names_its_session_to_the_commands_it_runs() {
        let account = Account { provider: Claude.provider(), label: "a".into(), home: PathBuf::from("/tmp/claude-a") };
        let request = LaunchRequest {
            verification: false,
            name: "worker".into(),
            provider: Claude.provider(),
            account: "a".into(),
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
        };
        let spec = Claude.host(&account, &request, Opening::Fresh(None), "").unwrap();
        assert!(spec.env.contains(&("NORTH_FLEET_SESSION".to_owned(), "worker".into())), "{:?}", spec.env);
    }
}
