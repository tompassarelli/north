use crate::platform::pyjson::{self, Value};
use std::io::{self, Read, Write};
use crate::hook_input::{HookInput, INPUT_LIMIT, Verdict};
use crate::fleet::runtime::Binding;

pub type GuardFn = fn(&HookInput) -> Verdict;

#[derive(Clone, Copy)]
pub struct Guard(&'static str, GuardFn);

pub const fn guard(unit: &'static str, decide: GuardFn) -> Guard {
    Guard(unit, decide)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum HookKind {
    PreBash,
    Safety,
    PreEdit,
    PreAgent,
    PreTimer,
    PreMessage,
    PreAsk,
    SpawnCapacity,
    NativeSpawn,
    PostTool,
    PermissionRequest,
    Stop,
    SubagentStop,
    RecommendActs,
    SessionStart,
    UserPromptSubmit,
    CodexConduct,
}

impl HookKind {
    fn explicit(kind: &str) -> Option<Self> {
        Some(match kind {
            "pre-bash" => Self::PreBash,
            "safety" => Self::Safety,
            "pre-edit" => Self::PreEdit,
            "pre-agent" => Self::PreAgent,
            "pre-timer" => Self::PreTimer,
            "pre-message" => Self::PreMessage,
            "pre-ask" => Self::PreAsk,
            "spawn-capacity" => Self::SpawnCapacity,
            "post-tool" => Self::PostTool,
            "permission-request" => Self::PermissionRequest,
            "stop" => Self::Stop,
            "subagent-stop" => Self::SubagentStop,
            "recommend-acts" => Self::RecommendActs,
            "session-start" => Self::SessionStart,
            "user-prompt-submit" => Self::UserPromptSubmit,
            "codex-conduct" => Self::CodexConduct,
            _ => return None,
        })
    }

    fn from_input(input: &HookInput) -> Option<Self> {
        Some(match input.data["hook_event_name"].as_str()? {
            "PreToolUse" => match input.tool_name() {
                name if crate::guard::shell_call::is_shell_tool(name) => Self::PreBash,
                "Edit" | "Write" | "MultiEdit" | "NotebookEdit" | "apply_patch" => Self::PreEdit,
                "Agent" | "spawn_agent" => Self::PreAgent,
                "ScheduleWakeup" | "CronCreate" => Self::PreTimer,
                crate::guard::subagent_message::MESSAGE_TOOL => Self::PreMessage,
                crate::guard::question_box::QUESTION_TOOL => Self::PreAsk,
                name if name.ends_with("spawn_agent") => Self::NativeSpawn,
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
            Self::PreBash | Self::Safety | Self::PreEdit | Self::PreAgent | Self::PreTimer | Self::PreMessage | Self::PreAsk | Self::SpawnCapacity | Self::NativeSpawn => "PreToolUse",
            Self::PostTool => "PostToolUse",
            Self::PermissionRequest => "PermissionRequest",
            Self::Stop | Self::RecommendActs => "Stop",
            Self::SubagentStop => "SubagentStop",
            Self::SessionStart => "SessionStart",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::CodexConduct => "*",
        }
    }

    fn guards(self) -> &'static [Guard] {
        match self {
            Self::PreBash => &[
                Guard("", crate::guard::tripwire::check),
                Guard("north-hook", crate::guard::proxy_role::evaluate),
                Guard("north-hook", crate::guard::proxy_role::evaluate_reads),
                Guard("corpus-scan-guard", crate::guard::corpus_scan::evaluate),
                Guard("resource-safe-search-guard", crate::guard::resource_safe_search::evaluate),
                Guard("modern-search-guard", crate::guard::modern_search::evaluate),
                Guard("north-hook", crate::guard::launch_critical::evaluate),
                Guard("north-hook", crate::guard::desktop_action::decide),
                Guard("north-hook", crate::guard::blind_stage::decide),
                Guard("north-hook", crate::guard::reviewer_cli::decide),
                Guard("north-hook", crate::guard::git_stash::decide),
                Guard("north-hook", crate::guard::worktree_add::decide),
                Guard("north-hook", crate::guard::worktree_remove::evaluate),
                Guard("north-hook", crate::guard::masked_pipeline::decide),
                Guard("north-hook", crate::guard::node_modules_write::decide),
                Guard("north-hook", crate::guard::native_evidence::evaluate),
                Guard("north-hook", crate::guard::issue_labels::evaluate),
                Guard("north-hook", crate::guard::issue_filing::evaluate),
                Guard("north-hook", crate::guard::issue_cleanup::close),
                Guard("north-hook", crate::guard::issue_shrink::evaluate),
                Guard("north-hook", crate::guard::sops_decrypt::evaluate),
                Guard("north-hook", crate::guard::public_text::evaluate),
                Guard("north-hook", crate::guard::frozen_topics::evaluate),
                Guard("poll-loop-guard", crate::guard::poll_loop::verdict),
                Guard("poll-loop-guard", crate::guard::poll_loop_task_output::verdict),
                Guard("heavy-command-guard", crate::guard::heavy_command::judge),
                Guard("", crate::guard::scratch_log::evaluate),
                Guard("session-kill-guard", crate::guard::session_kill::evaluate),
                Guard("clipboard-guard", crate::guard::clipboard::evaluate),
                Guard("worker-wait-guard", crate::guard::worker_wait::evaluate),
                Guard("native-launch-guard", crate::guard::native_launch::evaluate),
                Guard("native-item-guard", crate::guard::native_item::evaluate),
                Guard("north-hook", crate::guard::generated_policy::evaluate),
                Guard("north-hook", crate::guard::model_identity::evaluate),
                Guard("north-hook", crate::guard::nix_activity::evaluate),
            ],
            Self::Safety => &[Guard("", crate::guard::tripwire::check), Guard("north-hook", crate::guard::sops_decrypt::evaluate)],
            Self::PreEdit => &[
                Guard("north-hook", crate::guard::proxy_role::evaluate),
                Guard("north-hook", crate::guard::launch_critical::evaluate),
                Guard("north-hook", crate::guard::handoff_record::decide),
                Guard("north-hook", crate::guard::generated_policy::evaluate),
                Guard("north-hook", crate::guard::model_identity::evaluate),
                Guard("north-hook", crate::guard::file_skill::evaluate),
                Guard("north-hook", crate::guard::node_modules_write::decide),
                Guard("north-hook", crate::guard::nix_activity::evaluate),
            ],
            Self::PreAgent => &[Guard("north-hook", crate::guard::proxy_role::evaluate), Guard("", crate::guard::agent_cwd::evaluate), Guard("worker-tier-guard", crate::guard::worker_tier::evaluate), Guard("spawn-capacity-guard", crate::guard::spawn_capacity::admission)],
            Self::PreTimer => &[Guard("north-hook", crate::guard::lead_timer::evaluate)],
            Self::PreMessage => &[Guard("", crate::guard::subagent_message::evaluate)],
            Self::PreAsk => &[Guard("north-hook", crate::guard::question_box::evaluate)],
            Self::SpawnCapacity => &[Guard("", crate::guard::spawn_capacity::admission)],
            Self::NativeSpawn => &[Guard("north-hook", crate::guard::proxy_role::evaluate), Guard("spawn-capacity-guard", crate::guard::spawn_capacity::admission)],
            Self::PostTool => &[Guard("", crate::guard::accepted_finish::evaluate), Guard("north-hook", crate::guard::worker_lease::evaluate), Guard("north-hook", crate::guard::issue_filing::link), Guard("north-hook", crate::guard::code_health_edit::evaluate), Guard("north-hook", crate::guard::proxy_role::advise_reads)],
            Self::PermissionRequest => &[Guard("", crate::guard::tripwire::check)],
            Self::Stop => &[Guard("north-hook", crate::guard::permission_offer::evaluate), Guard("north-hook", crate::guard::permission_offer::evaluate_handoff), Guard("north-hook", crate::guard::recommendation_grounding::evaluate), Guard("north-hook", crate::guard::recommend_acts::evaluate), Guard("", crate::guard::correction_capture::evaluate)],
            Self::RecommendActs => &[Guard("north-hook", crate::guard::recommend_acts::evaluate)],
            Self::SubagentStop => &[Guard("north-hook", crate::guard::subagent_teardown::evaluate)],
            Self::SessionStart => &[Guard("north-hook", crate::guard::unlanded_work::evaluate), Guard("north-hook", crate::guard::compact_state::evaluate), Guard("north-hook", crate::guard::proxy_adopt::evaluate), Guard("north-hook", crate::guard::gate_digest::evaluate)],
            Self::UserPromptSubmit => &[Guard("", crate::guard::quiet_phrase::evaluate)],
            Self::CodexConduct => &[Guard("", crate::guard::codex_conduct::evaluate)],
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
    let mut blocks: Vec<String> = Vec::new();
    for guard in guards {
        if !guard.0.is_empty() && crate::guard::activation::authoring_guards_off(guard.0) {
            continue;
        }
        match (guard.1)(rewritten.as_ref().unwrap_or(input)) {
            Verdict::Allow => {}
            Verdict::Approve if event == "PermissionRequest" => {
                return Some(permission_decision("allow", None));
            }
            Verdict::Approve => {}
            Verdict::Deny(reason) if event == "PermissionRequest" => {
                return Some(permission_decision("deny", Some(reason)));
            }
            Verdict::Deny(reason) if matches!(event, "Stop" | "SubagentStop") => blocks.push(reason),
            Verdict::Deny(reason) if event == "PostToolUse" => {
                return Some(Value::object([("decision", "block".into()), ("reason", reason.clone().into()), ("continue", Value::Bool(false)), ("stopReason", reason.into())]));
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
            Verdict::End(reason) => {
                return Some(Value::object([("continue", Value::Bool(false)), ("stopReason", reason.into())]));
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
    if !blocks.is_empty() {
        return Some(Value::object([
            ("decision", "block".into()),
            ("reason", blocks.join("\n\n").into()),
        ]));
    }
    if matches!(event, "Stop" | "SubagentStop") {
        return (!context.is_empty()).then(|| Value::object([("systemMessage", context.join("\n").into())]));
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

/// The refusal north's Bash hook gives `command` run in `cwd`, for a session whose harness binds no hook: the worker sandbox screens each shell command of such a session with it, so a Pi or OpenCode worker meets the deciders a Claude worker's hook applies (north#1041).
pub fn command_refusal(command: &str, cwd: &std::path::Path) -> Option<String> {
    let tool_input = Value::object([("command", command.into())]);
    let data = Value::object([("hook_event_name", "PreToolUse".into()), ("tool_name", "Bash".into()), ("cwd", cwd.display().to_string().into()), ("tool_input", tool_input)]);
    let input = HookInput::parse(pyjson::dumps(&data).as_bytes())?;
    let output = evaluate(&input, HookKind::PreBash.guards(), HookKind::PreBash.event())?;
    let decision = &output["hookSpecificOutput"];
    (decision["permissionDecision"].as_str() == Some("deny")).then(|| decision["permissionDecisionReason"].as_str().unwrap_or("north hook refused this command").to_owned())
}

pub(crate) fn process(
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
        let event = if event == "*" { input.data["hook_event_name"].as_str().unwrap_or("") } else { event };
        let guards = spare_guards(guards, event, crate::fleet::runtime::binding(), || first_prompt(&input));
        if let Some(output) = evaluate(&input, &guards, event) {
            writer.write_all(pyjson::dumps(&output).as_bytes())?;
            writer.write_all(b"\n")?;
        }
    } else if bytes.len() as u64 <= INPUT_LIMIT && (event == "PreToolUse" || event.is_empty()) {
        crate::guard::launch_critical::record_invalid_input(&bytes);
    }
    Ok(())
}

/// The guards `event` runs in a session standing at `binding` (crate::fleet::runtime::binding): none at the start of a spare no launch has taken, whose context would be the spare's, and at a taken spare's `first` prompt the session-start guards before the prompt's own, so the context its start owes is produced for the session it was bound to.
fn spare_guards(guards: &[Guard], event: &str, binding: Binding, first: impl FnOnce() -> bool) -> Vec<Guard> {
    match (event, binding) {
        ("SessionStart", Binding::Awaited) => Vec::new(),
        ("UserPromptSubmit", Binding::Written) if first() => [HookKind::SessionStart.guards(), guards].concat(),
        _ => guards.to_vec(),
    }
}

/// Whether this prompt is the first of its conversation to reach the hook, marked once under the hooks' records.
fn first_prompt(input: &HookInput) -> bool {
    let (Some(conversation), Some(home)) = (input.data["session_id"].as_str().filter(|id| !id.is_empty() && !id.contains('/')), std::env::var_os("HOME")) else { return false };
    let marks = std::path::Path::new(&home).join(".local/state/agents/hooks/spare-starts");
    std::fs::create_dir_all(&marks).is_ok() && std::fs::OpenOptions::new().write(true).create_new(true).open(marks.join(conversation)).is_ok()
}

pub fn run(arguments: &[String]) -> Result<(), String> {
    let kind = match arguments {
        [] => None,
        [verb] if verb == "dispatch" => None,
        [verb] if verb == "spawn-gate" => {
            println!("{}", crate::guard::spawn_capacity::gate_summary(&crate::guard::spawn_capacity::Live));
            return Ok(());
        }
        [kind] => Some(HookKind::explicit(kind).ok_or_else(|| format!("unknown hook '{kind}'"))?),
        _ => return Err("Usage: north hook [stats [--since 7d]|dispatch|pre-bash|safety|pre-edit|post-tool|session-start|stop|recommend-acts|pre-agent|pre-message|spawn-capacity|spawn-gate|subagent-stop|user-prompt-submit|prompt-grounding|language-guard|firn-system-policy|permission-request|codex-conduct]".into()),
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
