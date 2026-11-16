use serde_json::{Value, json};
use std::io::{self, Read, Write};
use crate::hook_input::{Evidence, HookInput, INPUT_LIMIT, Verdict};

pub type GuardFn = fn(&HookInput) -> Verdict;

#[derive(Clone, Copy)]
pub struct Guard(&'static str, GuardFn);

pub const fn guard(unit: &'static str, decide: GuardFn) -> Guard {
    Guard(unit, decide)
}

/// What selects a hook from the input a harness sends: an event, a tool a `PreToolUse` call names, or a word any `spawn_agent` tool ends with.
#[derive(Clone, Copy)]
enum Answer {
    Never,
    Event(&'static str),
    Tool { event: &'static str, tools: &'static [&'static str] },
    Shell,
    Suffix(&'static str),
}

impl Answer {
    fn matches(&self, input: &HookInput) -> bool {
        match self {
            Self::Never => false,
            Self::Event(event) => provider(input) == Some(*event),
            Self::Tool { event, tools } => invokes(input, event, tools),
            Self::Shell => pre_bash(input),
            Self::Suffix(suffix) => pre_tool(input) && input.tool_name().ends_with(suffix),
        }
    }
}

/// One hook verb: the word `north hook` runs it under, empty for one only a harness's input selects, the provider event it answers, what input selects it, and the guards that judge in this order.
#[derive(Clone, Copy)]
pub struct Hook {
    pub name: &'static str,
    event: &'static str,
    guards: &'static [Guard],
    answers: Answer,
}

impl Hook {
    /// The hook named by `north hook NAME`.
    pub fn named(name: &str) -> Option<&'static Self> {
        HOOKS.iter().find(|hook| !hook.name.is_empty() && hook.name == name)
    }

    /// The hook that answers `input`; the table puts the tool-named hooks before the event-only ones they would otherwise answer.
    fn of(input: &HookInput) -> Option<&'static Self> {
        HOOKS.iter().find(|hook| hook.answers.matches(input))
    }

    fn guards(self) -> &'static [Guard] {
        self.guards
    }

    fn event(self) -> &'static str {
        self.event
    }
}

fn provider(input: &HookInput) -> Option<&str> {
    input.data["hook_event_name"].as_str()
}

fn pre_tool(input: &HookInput) -> bool {
    provider(input) == Some("PreToolUse")
}

fn invokes(input: &HookInput, event: &str, tools: &[&str]) -> bool {
    provider(input) == Some(event) && tools.contains(&input.tool_name())
}

fn pre_bash(input: &HookInput) -> bool {
    pre_tool(input) && crate::guard::shell_call::is_shell_tool(input.tool_name())
}

const PRE_BASH: &[Guard] = &[
    Guard("", crate::guard::tripwire::check),
    Guard("north-hook", crate::guard::coordinator_role::evaluate),
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
    Guard("north-hook", crate::guard::issue_close::close),
    Guard("north-hook", crate::guard::issue_shrink::evaluate),
    Guard("north-hook", crate::guard::sops_decrypt::evaluate),
    Guard("north-hook", crate::guard::public_text::evaluate),
    Guard("poll-loop-guard", crate::guard::poll_loop::verdict),
    Guard("poll-loop-guard", crate::guard::poll_loop_task_output::verdict),
    Guard("heavy-command-guard", crate::guard::heavy_command::judge),
    Guard("", crate::guard::scratch_log::evaluate),
    Guard("session-kill-guard", crate::guard::session_kill::evaluate),
    Guard("session-kill-guard", crate::guard::session_kill::evaluate_self_match),
    Guard("clipboard-guard", crate::guard::clipboard::evaluate),
    Guard("worker-wait-guard", crate::guard::worker_wait::evaluate),
    Guard("native-launch-guard", crate::guard::native_launch::evaluate),
    Guard("native-item-guard", crate::guard::native_item::evaluate),
    Guard("north-hook", crate::guard::generated_policy::evaluate),
    Guard("north-hook", crate::guard::model_identity::evaluate),
    Guard("north-hook", crate::guard::nix_activity::evaluate),
];

const SAFETY: &[Guard] = &[Guard("", crate::guard::tripwire::check), Guard("north-hook", crate::guard::sops_decrypt::evaluate)];

const PRE_EDIT: &[Guard] = &[
    Guard("north-hook", crate::guard::coordinator_role::evaluate),
    Guard("north-hook", crate::guard::launch_critical::evaluate),
    Guard("north-hook", crate::guard::handoff_record::decide),
    Guard("north-hook", crate::guard::generated_policy::evaluate),
    Guard("north-hook", crate::guard::model_identity::evaluate),
    Guard("north-hook", crate::guard::file_skill::evaluate),
    Guard("north-hook", crate::guard::node_modules_write::decide),
    Guard("north-hook", crate::guard::file_tool_reach::decide),
    Guard("north-hook", crate::guard::nix_activity::evaluate),
];

const PRE_READ: &[Guard] = &[
    Guard("", crate::guard::file_read_reach::decide),
    Guard("north-hook", crate::guard::coordinator_role::evaluate),
];

const PRE_AGENT: &[Guard] = &[
    Guard("north-hook", crate::guard::coordinator_role::evaluate),
    Guard("", crate::guard::agent_cwd::evaluate),
];



const PRE_ASK: &[Guard] = &[Guard("north-hook", crate::guard::question_box::evaluate)];

const NATIVE_SPAWN: &[Guard] = &[Guard("north-hook", crate::guard::coordinator_role::evaluate)];

const POST_TOOL: &[Guard] = &[
    Guard("", crate::guard::accepted_finish::evaluate),
    Guard("north-hook", crate::guard::code_health_edit::evaluate),
];

const PERMISSION_REQUEST: &[Guard] = &[Guard("", crate::guard::tripwire::check)];

const STOP: &[Guard] = &[
    Guard("north-hook", crate::guard::permission_offer::evaluate),
    Guard("north-hook", crate::guard::permission_offer::evaluate_handoff),
    Guard("north-hook", crate::guard::recommend_acts::evaluate),
    Guard("north-hook", crate::guard::unanswered::evaluate),
];

const RECOMMEND_ACTS: &[Guard] = &[Guard("north-hook", crate::guard::recommend_acts::evaluate)];

const SUBAGENT_STOP: &[Guard] = &[Guard("north-hook", crate::guard::subagent_teardown::evaluate)];

const SESSION_START: &[Guard] = &[
    Guard("north-hook", crate::guard::unlanded_work::evaluate),
    Guard("north-hook", crate::guard::gate_digest::evaluate),
];

const USER_PROMPT_SUBMIT: &[Guard] = &[Guard("", crate::guard::quiet_phrase::evaluate)];

const CODEX_CONDUCT: &[Guard] = &[Guard("", crate::guard::codex_conduct::evaluate)];

/// Every hook verb `north hook` names, which is the whole dispatch: a row's `answers` selects it from a harness's input, and its guards judge in order.
const HOOKS: &[Hook] = &[
    Hook { name: "pre-bash", event: "PreToolUse", guards: PRE_BASH, answers: Answer::Shell },
    Hook { name: "safety", event: "PreToolUse", guards: SAFETY, answers: Answer::Never },
    Hook { name: "pre-edit", event: "PreToolUse", guards: PRE_EDIT, answers: Answer::Tool { event: "PreToolUse", tools: &["Edit", "Write", "MultiEdit", "NotebookEdit", "apply_patch"] } },
    Hook { name: "pre-read", event: "PreToolUse", guards: PRE_READ, answers: Answer::Never },
    Hook { name: "pre-agent", event: "PreToolUse", guards: PRE_AGENT, answers: Answer::Tool { event: "PreToolUse", tools: &["Agent", "spawn_agent"] } },
    Hook { name: "pre-ask", event: "PreToolUse", guards: PRE_ASK, answers: Answer::Tool { event: "PreToolUse", tools: &[crate::guard::question_box::QUESTION_TOOL] } },
    Hook { name: "", event: "PreToolUse", guards: NATIVE_SPAWN, answers: Answer::Suffix("spawn_agent") },
    Hook { name: "post-tool", event: "PostToolUse", guards: POST_TOOL, answers: Answer::Event("PostToolUse") },
    Hook { name: "permission-request", event: "PermissionRequest", guards: PERMISSION_REQUEST, answers: Answer::Event("PermissionRequest") },
    Hook { name: "stop", event: "Stop", guards: STOP, answers: Answer::Event("Stop") },
    Hook { name: "subagent-stop", event: "SubagentStop", guards: SUBAGENT_STOP, answers: Answer::Event("SubagentStop") },
    Hook { name: "recommend-acts", event: "Stop", guards: RECOMMEND_ACTS, answers: Answer::Never },
    Hook { name: "session-start", event: "SessionStart", guards: SESSION_START, answers: Answer::Event("SessionStart") },
    Hook { name: "user-prompt-submit", event: "UserPromptSubmit", guards: USER_PROMPT_SUBMIT, answers: Answer::Event("UserPromptSubmit") },
    Hook { name: "codex-conduct", event: "*", guards: CODEX_CONDUCT, answers: Answer::Never },
];

fn permission_decision(behavior: &str, message: Option<String>) -> Value {
    let mut decision = json!({"behavior": behavior});
    if let Some(message) = message {
        decision["message"] = message.into();
    }
    json!({"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": decision}})
}

pub fn evaluate(input: &HookInput, guards: &[Guard], event: &str) -> Option<Value> {
    judge(input, guards, event, &mut Vec::new())
}

/// The hook output `guards` give `input`.
fn judge(input: &HookInput, guards: &[Guard], event: &str, evidence: &mut Vec<Evidence>) -> Option<Value> {
    let mut judged = Judged::default();
    let mut output = None;
    for guard in guards {
        if !guard.0.is_empty() && crate::guard::activation::authoring_guards_off(guard.0) {
            continue;
        }
        let verdict = (guard.1)(judged.rewritten.as_ref().unwrap_or(input));
        if let Some(ended) = judged.take(verdict, input, event) {
            output = Some(ended);
            break;
        }
    }
    evidence.append(&mut judged.evidence);
    output.or_else(|| judged.output(event))
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

/// The guards `name` names in the dispatch table, or none when no hook has that word.
fn guards_of(name: &str) -> &'static [Guard] {
    Hook::named(name).map_or(&[], |hook| hook.guards())
}

/// The refusal north's Bash hook gives `command` run in `cwd`, for a session whose harness binds no hook: the worker sandbox screens each shell command of such a session with it, so a Pi or OpenCode worker meets the deciders a Claude worker's hook applies (north#1041).
pub fn command_refusal(command: &str, cwd: &std::path::Path) -> (Option<String>, Vec<Evidence>) {
    let data = json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": cwd.display().to_string(), "tool_input": {"command": command}});
    let Some(input) = HookInput::parse(data.to_string().as_bytes()) else {
        return (None, Vec::new());
    };
    let pre_bash = Hook::named("pre-bash").expect("the dispatch table names the pre-bash hook");
    let mut evidence = Vec::new();
    let refusal = judge(&input, pre_bash.guards(), pre_bash.event(), &mut evidence).and_then(|output| {
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
    let mut bytes = Vec::new();
    if reader.take(INPUT_LIMIT * 4 + 1).read_to_end(&mut bytes).is_err() {
        return Ok(Vec::new());
    }
    let Some(input) = HookInput::parse(&bytes) else {
        notice_invalid_input(&bytes, event);
        return Ok(Vec::new());
    };
    let (guards, event) = match (event, Hook::of(&input)) {
        ("", None) => return Ok(Vec::new()),
        ("", Some(hook)) => (hook.guards(), hook.event()),
        (event, _) => (guards, event),
    };
    let event = if event == "*" { provider(&input).unwrap_or("") } else { event };
    let guards = spare_guards(guards, event, north_warm_spares::names::binding(), || first_prompt(&input));
    let mut evidence = Vec::new();
    if let Some(output) = judge(&input, &guards, event, &mut evidence) {
        writer.write_all(output.to_string().as_bytes())?;
        writer.write_all(b"\n")?;
    }
    Ok(evidence)
}

fn notice_invalid_input(bytes: &[u8], event: &str) {
    if bytes.len() as u64 <= INPUT_LIMIT && (event == "PreToolUse" || event.is_empty()) {
        crate::guard::launch_critical::record_invalid_input(bytes);
    }
}

/// The guards `event` runs in a session standing at `binding` (north_warm_spares::names::binding): none at the start of a spare no launch has taken, whose context would be the spare's, and at a taken spare's `first` prompt the session-start guards before the prompt's own, so the context its start owes is produced for the session it was bound to.
fn spare_guards(guards: &[Guard], event: &str, binding: north_warm_spares::names::Binding, first: impl FnOnce() -> bool) -> Vec<Guard> {
    match (event, binding) {
        ("SessionStart", north_warm_spares::names::Binding::Awaited) => Vec::new(),
        ("UserPromptSubmit", north_warm_spares::names::Binding::Written) if first() => [guards_of("session-start"), guards].concat(),
        _ => guards.to_vec(),
    }
}

/// Whether this prompt is the first of its conversation to reach the hook, marked once under the hooks' records.
fn first_prompt(input: &HookInput) -> bool {
    let (Some(conversation), Some(home)) = (input.data["session_id"].as_str().filter(|id| !id.is_empty() && !id.contains('/')), std::env::var_os("HOME")) else { return false };
    let marks = std::path::Path::new(&home).join(".local/state/agents/hooks/spare-starts");
    std::fs::create_dir_all(&marks).is_ok() && std::fs::OpenOptions::new().write(true).create_new(true).open(marks.join(conversation)).is_ok()
}

const USAGE: &str = "Usage: north hook [dispatch|pre-bash|safety|pre-edit|pre-read|post-tool|session-start|stop|recommend-acts|pre-agent|subagent-stop|user-prompt-submit|language-guard|firn-system-policy|permission-request|codex-conduct]";

pub fn run(arguments: &[String], request: &[u8], writer: &mut impl Write) -> Result<Vec<Evidence>, String> {
    let hook = match arguments {
        [] => None,
        [verb] if verb == "dispatch" => None,
        [name] => Some(Hook::named(name).ok_or_else(|| format!("unknown hook '{name}'"))?),
        _ => return Err(USAGE.into()),
    };
    let (guards, event) = hook.map_or((&[][..], ""), |hook| (hook.guards(), hook.event()));
    Ok(process(request, writer, guards, event).unwrap_or_default())
}

/// The answer for a call whose hook verb `arguments` missed its deadline: a reading tool call is refused, so a slow read guard never reads as an allow, and a heavy command is still routed through machine-capacity admission, so the deadline never lets it run unleased on this host.
pub fn deadline_answer(arguments: &[String], request: &[u8]) -> Option<Value> {
    if matches!(arguments, [name] if name == "pre-read") {
        return Some(json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": "file-read-reach: the read guard missed its deadline, so this read is refused; retry it."}}));
    }
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
