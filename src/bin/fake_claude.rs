use std::env;
use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use serde_json::{Value, json};

const SESSION: &str = "00000000-0000-4000-8000-0000000fa4e0";
const TOOL: &str = "toolu_fake";
const TASK: &str = "fake-task";
const USAGE: &str = "Usage: fake_claude --trace fold|done|idle-interrupt|cancel-queued|slow-hooks|signed-out-once|oauth-expired [ARG...]";
const STALL: &str = "stall this prompt";
const FINAL_REPORT: &str = "Done: fake report";
const SIGNED_OUT: &str = "Not logged in · Please run /login";
const EXPIRED: &str = "Failed to authenticate: OAuth session expired and could not be refreshed";

#[derive(Clone, Copy, Eq, PartialEq)]
enum Trace {
    Fold,
    Done,
    IdleInterrupt,
    CancelQueued,
    SlowHooks,
    SignedOutOnce,
    OAuthExpired,
}

impl Trace {
    fn parse(name: &str) -> Option<Self> {
        match name {
            "fold" => Some(Self::Fold),
            "done" => Some(Self::Done),
            "idle-interrupt" => Some(Self::IdleInterrupt),
            "cancel-queued" => Some(Self::CancelQueued),
            "slow-hooks" => Some(Self::SlowHooks),
            "signed-out-once" => Some(Self::SignedOutOnce),
            "oauth-expired" => Some(Self::OAuthExpired),
            _ => None,
        }
    }
}

struct Fake<W: Write> {
    trace: Trace,
    session: String,
    out: W,
    frames: u64,
    turn: Vec<String>,
    held: Vec<String>,
}

impl<W: Write> Fake<W> {
    fn handle(&mut self, frame: &Value) -> io::Result<bool> {
        match frame.get("type").and_then(Value::as_str) {
            Some("control_request") => self.control(frame),
            Some("user") => match frame.get("uuid").and_then(Value::as_str) {
                Some(uuid) => self.user(uuid, frame.get("message").cloned().unwrap_or(Value::Null)),
                None => Ok(false),
            },
            _ => Ok(false),
        }
    }

    fn control(&mut self, frame: &Value) -> io::Result<bool> {
        let id = frame.get("request_id").and_then(Value::as_str).unwrap_or_default().to_string();
        match frame.pointer("/request/subtype").and_then(Value::as_str) {
            Some("initialize") => {
                self.respond(&id, json!({"claude_code_version": "fake", "session_state": "idle"}))?;
                Ok(false)
            }
            Some("interrupt") => {
                self.respond(&id, json!({"still_queued": []}))?;
                if !self.turn.is_empty() {
                    self.abort()?;
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn user(&mut self, uuid: &str, message: Value) -> io::Result<bool> {
        self.lifecycle(uuid, "queued")?;
        if message.to_string().contains(STALL) && stall_once() {
            return Ok(false);
        }
        if self.trace == Trace::SignedOutOnce && signed_out_first() {
            return self.refuse(uuid, message, SIGNED_OUT);
        }
        if self.trace == Trace::OAuthExpired {
            return self.refuse(uuid, message, EXPIRED);
        }
        if self.turn.is_empty() {
            self.start(uuid, message)?;
            return Ok(false);
        }
        if self.trace != Trace::Fold {
            self.held.push(uuid.to_string());
            return Ok(false);
        }
        self.lifecycle(uuid, "started")?;
        self.replay(uuid, message)?;
        self.turn.push(uuid.to_string());
        self.emit(json!({"type": "system", "subtype": "task_notification", "task_id": TASK, "tool_use_id": TOOL, "status": "completed", "summary": "fake tool"}))?;
        self.reply(uuid, "OK")?;
        self.finish()?;
        Ok(true)
    }

    fn start(&mut self, uuid: &str, message: Value) -> io::Result<()> {
        self.lifecycle(uuid, "started")?;
        self.emit(json!({
            "type": "system",
            "subtype": "init",
            "model": "claude-fake",
            "claude_code_version": "fake",
            "permissionMode": "bypassPermissions",
            "capabilities": ["interrupt_receipt_v1", "interrupt_cancel_queued_v1", "msg_lifecycle_v1"],
        }))?;
        self.replay(uuid, message)?;
        self.turn.push(uuid.to_string());
        if self.trace == Trace::Done {
            self.reply(uuid, FINAL_REPORT)?;
            return self.finish();
        }
        if self.trace == Trace::IdleInterrupt {
            self.reply(uuid, "OK")?;
            return self.finish();
        }
        self.emit(json!({
            "type": "assistant",
            "parent_tool_use_id": null,
            "message": {"role": "assistant", "content": [{"type": "tool_use", "id": TOOL, "name": "Bash"}]},
            "user_message_uuids": [uuid],
        }))?;
        self.emit(json!({"type": "system", "subtype": "task_started", "task_id": TASK, "tool_use_id": TOOL, "description": "fake tool", "task_type": "local_bash", "is_backgrounded": false}))
    }

    fn refuse(&mut self, uuid: &str, message: Value, text: &str) -> io::Result<bool> {
        self.lifecycle(uuid, "started")?;
        self.emit(json!({"type": "system", "subtype": "init", "model": "claude-fake", "claude_code_version": "fake"}))?;
        self.replay(uuid, message)?;
        self.emit(json!({
            "type": "assistant",
            "parent_tool_use_id": null,
            "message": {"role": "assistant", "content": [{"type": "text", "text": text}]},
            "user_message_uuids": [uuid],
        }))?;
        self.emit(json!({"type": "result", "subtype": "success", "is_error": true, "result": text, "num_turns": 1, "user_message_uuids": [uuid]}))?;
        self.lifecycle(uuid, "completed")?;
        Ok(false)
    }

    fn finish(&mut self) -> io::Result<()> {
        let turn = std::mem::take(&mut self.turn);
        self.emit(json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "terminal_reason": "completed",
            "stop_reason": "end_turn",
            "result": "OK",
            "num_turns": 1,
            "user_message_uuids": turn,
        }))?;
        turn.iter().try_for_each(|uuid| self.lifecycle(uuid, "completed"))
    }

    fn abort(&mut self) -> io::Result<()> {
        let turn = std::mem::take(&mut self.turn);
        let held = std::mem::take(&mut self.held);
        self.emit(json!({"type": "system", "subtype": "task_notification", "task_id": TASK, "tool_use_id": TOOL, "status": "stopped", "summary": "fake tool"}))?;
        self.emit(json!({
            "type": "user",
            "parent_tool_use_id": null,
            "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": TOOL, "is_error": true}]},
        }))?;
        self.emit(json!({
            "type": "user",
            "parent_tool_use_id": null,
            "message": {"role": "user", "content": [{"type": "text", "text": "[Request interrupted by user for tool use]"}]},
        }))?;
        self.emit(json!({
            "type": "result",
            "subtype": "error_during_execution",
            "is_error": true,
            "terminal_reason": "aborted_tools",
            "stop_reason": "tool_use",
            "num_turns": 1,
            "user_message_uuids": turn,
        }))?;
        turn.iter().chain(&held).try_for_each(|uuid| self.lifecycle(uuid, "cancelled"))
    }

    fn reply(&mut self, uuid: &str, text: &str) -> io::Result<()> {
        self.emit(json!({
            "type": "assistant",
            "parent_tool_use_id": null,
            "message": {"role": "assistant", "content": [{"type": "text", "text": text}]},
            "user_message_uuids": [uuid],
        }))
    }

    fn replay(&mut self, uuid: &str, message: Value) -> io::Result<()> {
        self.emit(json!({"type": "user", "uuid": uuid, "isReplay": true, "parent_tool_use_id": null, "message": message}))
    }

    fn lifecycle(&mut self, uuid: &str, state: &str) -> io::Result<()> {
        self.emit(json!({"type": "command_lifecycle", "command_uuid": uuid, "state": state}))
    }

    fn respond(&mut self, id: &str, response: Value) -> io::Result<()> {
        let body = json!({"subtype": "success", "request_id": id, "response": response});
        writeln!(self.out, "{}", json!({"type": "control_response", "response": body}))?;
        self.out.flush()
    }

    fn emit(&mut self, mut frame: Value) -> io::Result<()> {
        self.frames += 1;
        frame["session_id"] = json!(self.session);
        if frame.get("uuid").is_none() {
            frame["uuid"] = json!(format!("00000000-0000-4000-8000-{:012x}", self.frames));
        }
        writeln!(self.out, "{frame}")?;
        self.out.flush()
    }
}

fn stall_once() -> bool {
    env::var_os("FAKE_CLAUDE_STALL_MARK").is_some_and(|mark| std::fs::OpenOptions::new().write(true).create_new(true).open(mark).is_ok())
}

fn signed_out_first() -> bool {
    env::var_os("FAKE_CLAUDE_SIGNED_OUT_MARK").is_some_and(|mark| std::fs::OpenOptions::new().write(true).create_new(true).open(mark).is_ok())
}

fn main() -> ExitCode {
    let arguments: Vec<String> = env::args().skip(1).collect();
    let trace = arguments
        .iter()
        .position(|argument| argument == "--trace")
        .and_then(|index| arguments.get(index + 1))
        .and_then(|name| Trace::parse(name));
    let Some(trace) = trace else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let session = arguments
        .iter()
        .position(|argument| argument == "--session-id" || argument == "--resume")
        .and_then(|index| arguments.get(index + 1))
        .map_or_else(|| SESSION.to_owned(), String::clone);
    if trace == Trace::SlowHooks {
        let hooks = env::var("FAKE_CLAUDE_HOOK_MS").ok().and_then(|ms| ms.parse().ok()).unwrap_or(1500);
        std::thread::sleep(std::time::Duration::from_millis(hooks));
    }
    let mut fake = Fake {
        trace,
        session,
        out: io::stdout().lock(),
        frames: 0,
        turn: Vec::new(),
        held: Vec::new(),
    };
    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(frame) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match fake.handle(&frame) {
            Ok(true) => break,
            Ok(false) => {}
            Err(_) => return ExitCode::FAILURE,
        }
    }
    ExitCode::SUCCESS
}
