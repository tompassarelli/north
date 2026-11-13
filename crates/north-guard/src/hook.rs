use serde_json::{Value, json};
use std::io::{self, Read, Write};
use crate::guard::container_shell;
use crate::hook_input::{Evidence, HookInput, INPUT_LIMIT, Verdict};
use crate::fleet::runtime::Binding;

pub type GuardFn = fn(&HookInput) -> Verdict;

#[derive(Clone, Copy)]
pub struct Guard(&'static str, GuardFn, Reach);

/// What a guard protects, which decides whether it judges a shell command that runs in a task container (`north fleet launch --container-shell`).
#[derive(Clone, Copy, PartialEq)]
enum Reach {
    /// This host's processes, files, desktop or build capacity, none of which a command in a task container touches.
    Host,
    /// What leaves the machine, credentials, or the session's own conduct, wherever its command runs.
    Anywhere,
}
use Reach::{Anywhere, Host};

pub const fn guard(unit: &'static str, decide: GuardFn) -> Guard {
    Guard(unit, decide, Anywhere)
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum HookKind {
    PreBash,
    Safety,
    PreEdit,
    PreRead,
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
            "pre-read" => Self::PreRead,
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
            Self::PreBash | Self::Safety | Self::PreEdit | Self::PreRead | Self::PreAgent | Self::PreTimer | Self::PreMessage | Self::PreAsk | Self::SpawnCapacity | Self::NativeSpawn => "PreToolUse",
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
                Guard("", crate::guard::tripwire::check, Anywhere),
                Guard("north-hook", crate::guard::proxy_role::evaluate, Anywhere),
                Guard("north-hook", crate::guard::proxy_role::evaluate_reads, Anywhere),
                Guard("corpus-scan-guard", crate::guard::corpus_scan::evaluate, Host),
                Guard("resource-safe-search-guard", crate::guard::resource_safe_search::evaluate, Host),
                Guard("modern-search-guard", crate::guard::modern_search::evaluate, Host),
                Guard("north-hook", crate::guard::launch_critical::evaluate, Host),
                Guard("north-hook", crate::guard::desktop_action::decide, Host),
                Guard("north-hook", crate::guard::blind_stage::decide, Host),
                Guard("north-hook", crate::guard::reviewer_cli::decide, Anywhere),
                Guard("north-hook", crate::guard::git_stash::decide, Host),
                Guard("north-hook", crate::guard::worktree_add::decide, Host),
                Guard("north-hook", crate::guard::worktree_remove::evaluate, Host),
                Guard("north-hook", crate::guard::masked_pipeline::decide, Anywhere),
                Guard("north-hook", crate::guard::node_modules_write::decide, Host),
                Guard("north-hook", crate::guard::native_evidence::evaluate, Host),
                Guard("north-hook", crate::guard::issue_labels::evaluate, Anywhere),
                Guard("north-hook", crate::guard::issue_filing::evaluate, Anywhere),
                Guard("north-hook", crate::guard::issue_close::close, Anywhere),
                Guard("north-hook", crate::guard::issue_shrink::evaluate, Anywhere),
                Guard("north-hook", crate::guard::sops_decrypt::evaluate, Anywhere),
                Guard("north-hook", crate::guard::public_text::evaluate, Anywhere),
                Guard("north-hook", crate::guard::frozen_topics::evaluate, Anywhere),
                Guard("poll-loop-guard", crate::guard::poll_loop::verdict, Anywhere),
                Guard("poll-loop-guard", crate::guard::poll_loop_task_output::verdict, Anywhere),
                Guard("heavy-command-guard", crate::guard::heavy_command::judge, Host),
                Guard("", crate::guard::scratch_log::evaluate, Host),
                Guard("session-kill-guard", crate::guard::session_kill::evaluate, Host),
                Guard("session-kill-guard", crate::guard::session_kill::evaluate_self_match, Anywhere),
                Guard("clipboard-guard", crate::guard::clipboard::evaluate, Host),
                Guard("worker-wait-guard", crate::guard::worker_wait::evaluate, Anywhere),
                Guard("native-launch-guard", crate::guard::native_launch::evaluate, Host),
                Guard("native-item-guard", crate::guard::native_item::evaluate, Anywhere),
                Guard("north-hook", crate::guard::generated_policy::evaluate, Host),
                Guard("north-hook", crate::guard::model_identity::evaluate, Anywhere),
                Guard("north-hook", crate::guard::nix_activity::evaluate, Host),
            ],
            Self::Safety => &[Guard("", crate::guard::tripwire::check, Anywhere), Guard("north-hook", crate::guard::sops_decrypt::evaluate, Anywhere)],
            Self::PreEdit => &[
                Guard("north-hook", crate::guard::proxy_role::evaluate, Anywhere),
                Guard("north-hook", crate::guard::launch_critical::evaluate, Host),
                Guard("north-hook", crate::guard::handoff_record::decide, Anywhere),
                Guard("north-hook", crate::guard::generated_policy::evaluate, Host),
                Guard("north-hook", crate::guard::model_identity::evaluate, Anywhere),
                Guard("north-hook", crate::guard::file_skill::evaluate, Anywhere),
                Guard("north-hook", crate::guard::node_modules_write::decide, Host),
                Guard("north-hook", crate::guard::file_tool_reach::decide, Host),
                Guard("north-hook", crate::guard::nix_activity::evaluate, Host),
            ],
            Self::PreRead => &[Guard("", crate::guard::file_read_reach::decide, Host)],
            Self::PreAgent => &[Guard("north-hook", crate::guard::proxy_role::evaluate, Anywhere), Guard("", crate::guard::agent_cwd::evaluate, Anywhere), Guard("worker-tier-guard", crate::guard::worker_tier::evaluate, Anywhere), Guard("spawn-capacity-guard", crate::guard::spawn_capacity::admission, Anywhere)],
            Self::PreTimer => &[Guard("north-hook", crate::guard::lead_timer::evaluate, Anywhere)],
            Self::PreMessage => &[Guard("", crate::guard::subagent_message::evaluate, Anywhere)],
            Self::PreAsk => &[Guard("north-hook", crate::guard::question_box::evaluate, Anywhere)],
            Self::SpawnCapacity => &[Guard("", crate::guard::spawn_capacity::admission, Anywhere)],
            Self::NativeSpawn => &[Guard("north-hook", crate::guard::proxy_role::evaluate, Anywhere), Guard("spawn-capacity-guard", crate::guard::spawn_capacity::admission, Anywhere)],
            Self::PostTool => &[Guard("", crate::guard::accepted_finish::evaluate, Anywhere), Guard("north-hook", crate::guard::worker_lease::evaluate, Anywhere), Guard("north-hook", crate::guard::issue_filing::link, Anywhere), Guard("north-hook", crate::guard::code_health_edit::evaluate, Anywhere), Guard("north-hook", crate::guard::proxy_role::advise_reads, Anywhere)],
            Self::PermissionRequest => &[Guard("", crate::guard::tripwire::check, Anywhere)],
            Self::Stop => &[Guard("north-hook", crate::guard::permission_offer::evaluate, Anywhere), Guard("north-hook", crate::guard::permission_offer::evaluate_handoff, Anywhere), Guard("north-hook", crate::guard::recommendation_grounding::evaluate, Anywhere), Guard("north-hook", crate::guard::recommend_acts::evaluate, Anywhere), Guard("", crate::guard::correction_capture::evaluate, Anywhere)],
            Self::RecommendActs => &[Guard("north-hook", crate::guard::recommend_acts::evaluate, Anywhere)],
            Self::SubagentStop => &[Guard("north-hook", crate::guard::subagent_teardown::evaluate, Anywhere)],
            Self::SessionStart => &[Guard("north-hook", crate::guard::unlanded_work::evaluate, Anywhere), Guard("north-hook", crate::guard::compact_state::evaluate, Anywhere), Guard("north-hook", crate::guard::proxy_adopt::evaluate, Anywhere), Guard("north-hook", crate::guard::gate_digest::evaluate, Anywhere)],
            Self::UserPromptSubmit => &[Guard("", crate::guard::quiet_phrase::evaluate, Anywhere)],
            Self::CodexConduct => &[Guard("", crate::guard::codex_conduct::evaluate, Anywhere)],
        }
    }
}

fn permission_decision(behavior: &str, message: Option<String>) -> Value {
    let mut decision = json!({"behavior": behavior});
    if let Some(message) = message {
        decision["message"] = message.into();
    }
    json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": decision}})
}

pub fn evaluate(input: &HookInput, guards: &[Guard], event: &str) -> Option<Value> {
    judge(input, guards, event, || None, &mut Vec::new(), &mut Vec::new())
}

/// [`evaluate`] for the session this hook runs in: when its launch declared a container shell, each verdict that stands down is written to the hooks' records.
fn evaluate_here(input: &HookInput, guards: &[Guard], event: &str, evidence: &mut Vec<Evidence>) -> Option<Value> {
    let mut stood_down = Vec::new();
    let output = judge(input, guards, event, container_shell::declared, &mut stood_down, evidence);
    container_shell::record(input, &stood_down);
    output
}

/// The hook output `guards` give `input`. For a file tool call of a session `in_container` that names the container directory its own directory is mounted at, a path under it is first moved to the same place under the session's own directory. For a shell command of a session `in_container`, a [`Host`] guard's refusal or advice and any guard's rewrite do not apply, since the command touches nothing on this host and a rewrite names a tool only this host has; what each would have said goes to `stood_down`.
fn judge(input: &HookInput, guards: &[Guard], event: &str, in_container: impl FnOnce() -> Option<container_shell::ContainerSession>, stood_down: &mut Vec<String>, evidence: &mut Vec<Evidence>) -> Option<Value> {
    let in_container = std::cell::LazyCell::new(in_container);
    let mut judged = Judged::default();
    judged.take(container_shell::moved_file_call(event, input, &in_container), input, event);
    let mut output = None;
    for guard in guards {
        if !guard.0.is_empty() && crate::guard::activation::authoring_guards_off(guard.0) {
            continue;
        }
        let verdict = (guard.1)(judged.rewritten.as_ref().unwrap_or(input));
        if let Some(said) = host_bound(*guard, &verdict).filter(|_| input.is_shell() && in_container.is_some()) {
            stood_down.push(said.to_owned());
        } else if let Some(ended) = judged.take(verdict, input, event) {
            output = Some(ended);
            break;
        }
    }
    evidence.append(&mut judged.evidence);
    output.or_else(|| judged.output(event))
}

/// What `verdict` says when it holds only for a command on this host: any rewrite, and a [`Host`] guard's refusal or advice.
fn host_bound(guard: Guard, verdict: &Verdict) -> Option<&str> {
    match verdict {
        Verdict::Rewrite(_, said) => Some(said.as_str()),
        Verdict::Deny(said) | Verdict::Advise(said) if guard.2 == Host => Some(said.as_str()),
        _ => None,
    }
}

/// The verdicts taken so far for one hook input.
#[derive(Default)]
struct Judged {
    context: Vec<String>,
    updated_input: Option<Value>,
    rewritten: Option<HookInput>,
    blocks: Vec<String>,
    evidence: Vec<Evidence>,
}

impl Judged {
    /// Takes one guard's verdict, returning the hook output when it ends the judging.
    fn take(&mut self, verdict: Verdict, input: &HookInput, event: &str) -> Option<Value> {
        match verdict {
            Verdict::Allow => None,
            Verdict::Record(row) => {
                self.evidence.push(row);
                None
            }
            Verdict::Approve => (event == "PermissionRequest").then(|| permission_decision("allow", None)),
            Verdict::Deny(reason) => self.refuse(reason, event),
            Verdict::End(reason) => Some(json!({"continue": false, "stopReason": reason})),
            Verdict::Advise(text) => {
                self.context.push(text);
                None
            }
            Verdict::Rewrite(updated, text) => {
                let mut data = input.data.clone();
                data["tool_input"] = updated.clone();
                self.rewritten = HookInput::parse(data.to_string().as_bytes());
                self.updated_input = Some(updated);
                self.context.push(text);
                None
            }
        }
    }

    fn refuse(&mut self, reason: String, event: &str) -> Option<Value> {
        match event {
            "PermissionRequest" => Some(permission_decision("deny", Some(reason))),
            "Stop" | "SubagentStop" => {
                self.blocks.push(reason);
                None
            }
            "PostToolUse" => Some(json!({"decision": "block", "reason": reason, "continue": false, "stopReason": reason})),
            _ => Some(json!({"hookSpecificOutput": {"hookEventName": event, "permissionDecision": "deny", "permissionDecisionReason": reason}})),
        }
    }

    fn output(self, event: &str) -> Option<Value> {
        if !self.blocks.is_empty() {
            return Some(json!({"decision": "block", "reason": self.blocks.join("\n\n")}));
        }
        if matches!(event, "Stop" | "SubagentStop") {
            return (!self.context.is_empty()).then(|| json!({"systemMessage": self.context.join("\n")}));
        }
        let mut output = json!({"hookEventName": event});
        if let Some(updated) = self.updated_input {
            output["permissionDecision"] = "allow".into();
            output["updatedInput"] = updated;
        }
        if !self.context.is_empty() {
            output["additionalContext"] = self.context.join("\n").into();
        }
        (output.as_object()?.len() > 1).then(|| json!({"hookSpecificOutput": output}))
    }
}

/// The refusal north's Bash hook gives `command` run in `cwd`, for a session whose harness binds no hook: the worker sandbox screens each shell command of such a session with it, so a Pi or OpenCode worker meets the deciders a Claude worker's hook applies (north#1041).
pub fn command_refusal(command: &str, cwd: &std::path::Path) -> (Option<String>, Vec<Evidence>) {
    let data = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": cwd.display().to_string(), "tool_input": {"command": command}});
    let Some(input) = HookInput::parse(data.to_string().as_bytes()) else {
        return (None, Vec::new());
    };
    let mut evidence = Vec::new();
    let refusal = evaluate_here(&input, HookKind::PreBash.guards(), HookKind::PreBash.event(), &mut evidence).and_then(|output| {
        let decision = &output["hookSpecificOutput"];
        (decision["permissionDecision"].as_str() == Some("deny")).then(|| decision["permissionDecisionReason"].as_str().unwrap_or("north hook refused this command").to_owned())
    });
    (refusal, evidence)
}

pub(crate) fn process(
    reader: impl Read,
    writer: &mut impl Write,
    guards: &[Guard],
    event: &str,
) -> io::Result<Vec<Evidence>> {
    let mut evidence = Vec::new();
    let mut bytes = Vec::new();
    if reader
        .take(INPUT_LIMIT * 4 + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Ok(evidence);
    }
    if let Some(input) = HookInput::parse(&bytes) {
        let (guards, event) = if event.is_empty() {
            let Some(kind) = HookKind::from_input(&input) else {
                return Ok(evidence);
            };
            (kind.guards(), kind.event())
        } else {
            (guards, event)
        };
        let event = if event == "*" { input.data["hook_event_name"].as_str().unwrap_or("") } else { event };
        let guards = spare_guards(guards, event, crate::fleet::runtime::binding(), || first_prompt(&input));
        if let Some(output) = evaluate_here(&input, &guards, event, &mut evidence) {
            writer.write_all(output.to_string().as_bytes())?;
            writer.write_all(b"\n")?;
        }
    } else if bytes.len() as u64 <= INPUT_LIMIT && (event == "PreToolUse" || event.is_empty()) {
        crate::guard::launch_critical::record_invalid_input(&bytes);
    }
    Ok(evidence)
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

pub fn run(arguments: &[String], request: &[u8], writer: &mut impl Write) -> Result<Vec<Evidence>, String> {
    let kind = match arguments {
        [] => None,
        [verb] if verb == "dispatch" => None,
        [verb] if verb == "spawn-gate" => {
            let _ = writeln!(writer, "{}", crate::guard::spawn_capacity::gate_summary(&crate::guard::spawn_capacity::Live));
            return Ok(Vec::new());
        }
        [kind] => Some(HookKind::explicit(kind).ok_or_else(|| format!("unknown hook '{kind}'"))?),
        _ => return Err("Usage: north hook [stats [--since 7d]|dispatch|pre-bash|safety|pre-edit|pre-read|post-tool|session-start|stop|recommend-acts|pre-agent|pre-message|spawn-capacity|spawn-gate|subagent-stop|user-prompt-submit|prompt-grounding|language-guard|firn-system-policy|permission-request|codex-conduct]".into()),
    };
    Ok(process(
        request,
        writer,
        kind.map_or(&[], HookKind::guards),
        kind.map_or("", HookKind::event),
    )
    .unwrap_or_default())
}

/// The answer for a Bash call whose hook missed its deadline: a heavy command is still routed through machine-capacity admission, so the deadline never lets it run unleased on this host.
pub fn deadline_answer(request: &[u8]) -> Option<Value> {
    let input = HookInput::parse(request)?;
    if input.data["tool_name"].as_str() != Some("Bash") {
        return None;
    }
    match crate::guard::heavy_command::judge(&input) {
        Verdict::Rewrite(updated, said) => Some(json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow", "updatedInput": updated, "additionalContext": said}})),
        Verdict::Deny(reason) => Some(json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}})),
        _ => None,
    }
}

#[cfg(test)]
#[path = "hook_tests.rs"]
mod tests;
