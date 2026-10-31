use crate::platform::pyjson::{self, Value};
use std::io::{self, Read, Write};

const INPUT_LIMIT: u64 = 1024 * 1024;

#[derive(Debug)]
pub struct HookInput {
    pub data: Value,
    pub tool_input: Value,
    pub raw: Vec<u8>,
    command: Option<pyjson::PyString>,
}

impl HookInput {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if std::str::from_utf8(bytes).ok()?.chars().count() as u64 > INPUT_LIMIT {
            return None;
        }
        let data = pyjson::loads(bytes).ok()?;
        data.as_object()?;
        let patch = data["tool_name"]
            .as_str()
            .is_some_and(|tool| tool.ends_with("apply_patch"));
        let tool_input = match data.get("tool_input") {
            Some(Value::String(text)) => match pyjson::loads_string(text) {
                Ok(value) => value,
                Err(_) if patch => Value::String(text.clone()),
                Err(_) => return None,
            },
            Some(value) => value.clone(),
            None => Value::object([]),
        };
        if !patch && !tool_input.is_object() && !tool_input.is_null() {
            return None;
        }
        let command = crate::guard::shell_call::command_text(data["tool_name"].as_str().unwrap_or(""), &tool_input);
        Some(Self {
            data,
            tool_input,
            raw: bytes.to_vec(),
            command,
        })
    }

    pub fn tool_name(&self) -> &str {
        self.data["tool_name"]
            .as_str()
            .unwrap_or("")
            .rsplit('.')
            .next()
            .unwrap_or("")
    }

    pub fn is_shell(&self) -> bool {
        crate::guard::shell_call::is_shell_tool(self.tool_name())
    }

    pub fn command(&self) -> Option<&pyjson::PyString> {
        self.command.as_ref()
    }

    pub fn cwd(&self) -> Option<&str> {
        self.tool_input["workdir"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| self.data["cwd"].as_str())
    }
}

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Allow,
    Deny(String),
    Rewrite(Value, String),
    Advise(String),
    Approve,
}

pub type Guard = fn(&HookInput) -> Verdict;

#[derive(Debug, Clone, Copy, PartialEq)]
enum HookKind {
    PreBash,
    PreEdit,
    PreAgent,
    PostTool,
    PermissionRequest,
    Stop,
    SubagentStop,
    SessionStart,
    UserPromptSubmit,
}

impl HookKind {
    fn explicit(kind: &str) -> Option<Self> {
        Some(match kind {
            "pre-bash" => Self::PreBash,
            "pre-edit" => Self::PreEdit,
            "pre-agent" => Self::PreAgent,
            "post-tool" => Self::PostTool,
            "permission-request" => Self::PermissionRequest,
            "stop" => Self::Stop,
            "subagent-stop" => Self::SubagentStop,
            "session-start" => Self::SessionStart,
            "user-prompt-submit" => Self::UserPromptSubmit,
            _ => return None,
        })
    }

    fn from_input(input: &HookInput) -> Option<Self> {
        Some(match input.data["hook_event_name"].as_str()? {
            "PreToolUse" => match input.tool_name() {
                name if crate::guard::shell_call::is_shell_tool(name) => Self::PreBash,
                "Edit" | "Write" | "MultiEdit" | "NotebookEdit" | "apply_patch" => Self::PreEdit,
                "Agent" | "spawn_agent" => Self::PreAgent,
                _ => return None,
            },
            "PostToolUse" => Self::PostTool,
            "PermissionRequest" => Self::PermissionRequest,
            "Stop" => Self::Stop,
            "SubagentStop" => Self::SubagentStop,
            "SessionStart" => Self::SessionStart,
            "UserPromptSubmit" => Self::UserPromptSubmit,
            _ => return None,
        })
    }

    fn event(self) -> &'static str {
        match self {
            Self::PreBash | Self::PreEdit | Self::PreAgent => "PreToolUse",
            Self::PostTool => "PostToolUse",
            Self::PermissionRequest => "PermissionRequest",
            Self::Stop => "Stop",
            Self::SubagentStop => "SubagentStop",
            Self::SessionStart => "SessionStart",
            Self::UserPromptSubmit => "UserPromptSubmit",
        }
    }

    fn guards(self) -> &'static [Guard] {
        match self {
            Self::PreBash => &[
                crate::guard::tripwire::check,
                crate::guard::corpus_scan::evaluate,
                crate::guard::resource_safe_search::evaluate,
                crate::guard::modern_search::evaluate,
                crate::guard::launch_critical::evaluate,
                crate::guard::desktop_action::decide,
                crate::guard::blind_stage::evaluate,
                crate::guard::reviewer_cli::evaluate,
                crate::guard::git_stash::evaluate,
                crate::guard::native_evidence::evaluate,
                crate::guard::issue_labels::evaluate,
                crate::guard::issue_filing::evaluate,
                crate::guard::sops_decrypt::evaluate,
                crate::guard::public_text::evaluate,
                crate::guard::frozen_topics::evaluate,
                crate::guard::poll_loop::evaluate,
                crate::guard::heavy_command::evaluate,
                crate::guard::session_kill::evaluate,
                crate::guard::clipboard::evaluate,
                crate::guard::worker_wait::evaluate,
                crate::guard::native_launch::evaluate,
                crate::guard::native_item::evaluate,
                crate::guard::generated_policy::evaluate,
                crate::guard::model_identity::evaluate,
            ],
            Self::PreEdit => &[
                crate::guard::launch_critical::evaluate,
                crate::guard::handoff_record::evaluate,
                crate::guard::generated_policy::evaluate,
                crate::guard::model_identity::evaluate,
                crate::guard::file_skill::evaluate,
            ],
            Self::PreAgent => &[crate::guard::worker_tier::evaluate],
            Self::PostTool => &[crate::guard::worker_handoff::evaluate, crate::guard::issue_filing::link, crate::guard::code_health_edit::evaluate],
            Self::PermissionRequest => &[crate::guard::tripwire::check],
            Self::Stop => &[],
            Self::SubagentStop => &[crate::guard::subagent_teardown::evaluate],
            Self::SessionStart => &[crate::guard::unlanded_work::evaluate],
            Self::UserPromptSubmit => &[],
        }
    }
}

fn permission_decision(behavior: &str, message: Option<String>) -> Value {
    let mut decision = Value::object([("behavior", behavior.into())]);
    if let Some(message) = message {
        decision["message"] = message.into();
    }
    Value::object([(
        "hookSpecificOutput",
        Value::object([
            ("hookEventName", "PermissionRequest".into()),
            ("decision", decision),
        ]),
    )])
}

pub fn evaluate(input: &HookInput, guards: &[Guard], event: &str) -> Option<Value> {
    let mut context = Vec::new();
    let mut updated_input = None;
    let mut rewritten = None;
    for guard in guards {
        match guard(rewritten.as_ref().unwrap_or(input)) {
            Verdict::Allow => {}
            Verdict::Approve if event == "PermissionRequest" => {
                return Some(permission_decision("allow", None));
            }
            Verdict::Approve => {}
            Verdict::Deny(reason) if event == "PermissionRequest" => {
                return Some(permission_decision("deny", Some(reason)));
            }
            Verdict::Deny(reason) if matches!(event, "Stop" | "SubagentStop") => {
                return Some(Value::object([
                    ("decision", "block".into()),
                    ("reason", reason.into()),
                ]));
            }
            Verdict::Deny(reason) => {
                return Some(Value::object([(
                    "hookSpecificOutput",
                    Value::object([
                        ("hookEventName", event.into()),
                        ("permissionDecision", "deny".into()),
                        ("permissionDecisionReason", reason.into()),
                    ]),
                )]));
            }
            Verdict::Advise(text) => context.push(text),
            Verdict::Rewrite(updated, text) => {
                let mut data = input.data.clone();
                data["tool_input"] = updated.clone();
                rewritten = HookInput::parse(pyjson::dumps(&data).as_bytes());
                updated_input = Some(updated);
                context.push(text);
            }
        }
    }
    let mut output = Value::object([("hookEventName", event.into())]);
    if let Some(updated) = updated_input {
        output["permissionDecision"] = "allow".into();
        output["updatedInput"] = updated;
    }
    if !context.is_empty() {
        output["additionalContext"] = context.join("\n").into();
    }
    (output.as_object()?.len() > 1).then(|| Value::object([("hookSpecificOutput", output)]))
}

fn process(
    reader: impl Read,
    writer: &mut impl Write,
    guards: &[Guard],
    event: &str,
) -> io::Result<()> {
    let mut bytes = Vec::new();
    if reader
        .take(INPUT_LIMIT * 4 + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Ok(());
    }
    if let Some(input) = HookInput::parse(&bytes) {
        let (guards, event) = if event.is_empty() {
            let Some(kind) = HookKind::from_input(&input) else {
                return Ok(());
            };
            (kind.guards(), kind.event())
        } else {
            (guards, event)
        };
        if let Some(output) = evaluate(&input, guards, event) {
            writer.write_all(pyjson::dumps(&output).as_bytes())?;
            writer.write_all(b"\n")?;
        }
    } else if bytes.len() as u64 <= INPUT_LIMIT && (event == "PreToolUse" || event.is_empty()) {
        crate::guard::launch_critical::record_invalid_input(&bytes);
    }
    Ok(())
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let kind = match arguments {
        [] => None,
        [kind] => Some(HookKind::explicit(kind).ok_or_else(|| format!("unknown hook '{kind}'"))?),
        _ => return Err("Usage: north hook [pre-bash|pre-edit|post-tool|session-start|stop|pre-agent|subagent-stop|user-prompt-submit|permission-request]".into()),
    };
    let _ = process(
        io::stdin().lock(),
        &mut io::stdout().lock(),
        kind.map_or(&[], HookKind::guards),
        kind.map_or("", HookKind::event),
    );
    Ok(())
}

#[cfg(test)]
#[path = "hook_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "hook_rewrite_tests.rs"]
mod rewrite_tests;
