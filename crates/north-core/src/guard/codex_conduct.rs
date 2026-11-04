mod bash;
mod gates;
mod plan;
mod post;
mod state;
mod stop;

use crate::hook::{HookInput, Verdict};
use regex::Regex;
use serde_json::Value;
use std::path::{Path, PathBuf};


pub(crate) struct Ctx {
    pub state_root: PathBuf,
    pub code_root: PathBuf,
    pub now: f64,
}

impl Ctx {
    fn from_env() -> Self {
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        let home = var("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
        let runtime = var("XDG_RUNTIME_DIR").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from);
        Self {
            state_root: std::env::var_os("CODEX_BEHAVIOR_STATE").map_or_else(|| runtime.join("codex-behavior"), PathBuf::from),
            code_root: std::env::var_os("CODEX_CONDUCT_CODE_ROOT").map_or_else(|| home.join("code"), PathBuf::from),
            now: north_base::foundation::clock::Timestamp::now().unix_seconds_f64(),
        }
    }
}

pub(super) fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("codex conduct pattern compiles")
}

macro_rules! pattern {
    ($name:ident, $pattern:expr) => {
        pub(super) static $name: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| $crate::guard::codex_conduct::re($pattern));
    };
}
pub(crate) use pattern;

pub(super) const PEER_LINE: &str = "A real doubt goes to your parent through north fleet ask.";
const TOOLING_REPOS: [&str; 3] = ["nixos-config", "north", "fram"];
const PROFILES: [&str; 3] = ["prototype", "tooling", "client"];

pattern!(CORRECTION, r"(?i)\b(fuck\w*|shit\w*|wtf|ffs|jesus|stop (asking|doing|with|adding|it)|(i|i've) (already )?(told|asked) you|you keep|again\?|why (are|do|did) you|for the (second|third|fourth) time|not what i asked|too much (process|ceremony)|over-?engineer\w*)\b");
pattern!(CLOSE_REQUEST, r"(?i)\b(not (done|closed|finished|landed) yet|(isn't|is not|still not) (done|closed|finished|landed)|still open|close (it|this|that|the issue|#?\d+)|land (it|this|the plane))\b");
pattern!(NEGATION, r"\b(no|not|don't|dont|stop|without|never|skip|drop|enough)\b[^.!?\n]{0,30}$");
pattern!(PUNCTUATION, r"[^\w\s]");
pattern!(PROFILE_LINE, r"(?m)^profile:\s*(\w+)");

pub(super) fn normalize(text: &str) -> String {
    PUNCTUATION.replace_all(&text.to_lowercase(), " ").split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(super) fn asked_keyword(keywords: &[&str], prompts: &[String]) -> bool {
    prompts.iter().filter(|prompt| !CORRECTION.is_match(prompt)).any(|prompt| {
        let text = normalize(prompt);
        keywords
            .iter()
            .any(|keyword| text.match_indices(keyword).any(|(start, _)| !NEGATION.is_match(&text[..start])))
    })
}

pub(super) fn quoted_in(quote: &str, prompts: &[String]) -> bool {
    let needle = normalize(quote);
    needle.split_whitespace().count() >= 3 && normalize(&prompts.join(" ")).contains(&needle)
}

pub(super) fn expected_profile(ctx: &Ctx, cwd: Option<&str>) -> &'static str {
    let path = state::resolve(Path::new(cwd.filter(|c| !c.is_empty()).unwrap_or(".")));
    for directory in path.ancestors() {
        if let Ok(text) = std::fs::read(directory.join("AGENTS.md")) {
            let text = String::from_utf8_lossy(&text);
            if let Some(found) = PROFILE_LINE.captures(&text).and_then(|m| PROFILES.into_iter().find(|p| *p == &m[1])) {
                return found;
            }
        }
        if directory == ctx.code_root {
            break;
        }
    }
    let Ok(relative) = path.strip_prefix(&ctx.code_root) else {
        return "prototype";
    };
    match relative.components().next().map(|part| part.as_os_str().to_string_lossy().into_owned()) {
        Some(first) if first == "clients" => "client",
        Some(first) if TOOLING_REPOS.contains(&first.as_str()) => "tooling",
        _ => "prototype",
    }
}

pub(super) fn minutes(seconds: f64) -> String {
    let total = (seconds / 60.0).floor() as i64;
    if total >= 60 { format!("{}h {:02}m", total / 60, total % 60) } else { format!("{total} min") }
}

pub(super) fn clock(at: f64) -> String {
    north_base::foundation::clock::Timestamp::from_unix_seconds_f64(at).local_hour_minute()
}

pub(super) fn chars(text: &str, limit: usize) -> &str {
    text.char_indices().nth(limit).map_or(text, |(end, _)| &text[..end])
}

pub(super) fn log_override(ctx: &Ctx, input: &HookInput, gate: &str, case: &str, fact: &str) {
    use crate::platform::pyjson::{self, Value as Py};
    let line = Py::object([
        ("at", Py::float(ctx.now)),
        ("session", input.data["session_id"].clone()),
        ("agent", input.data["agent_id"].clone()),
        ("gate", gate.into()),
        ("case", case.into()),
        ("cwd", input.data["cwd"].clone()),
        ("fact", chars(fact, 300).into()),
    ]);
    let _ = std::fs::create_dir_all(&ctx.state_root);
    if let Ok(mut log) = std::fs::OpenOptions::new().create(true).append(true).open(ctx.state_root.join("verify-overrides.jsonl")) {
        use std::io::Write;
        let _ = writeln!(log, "{}", pyjson::dumps(&line));
    }
}

pub(super) fn command(event: &Value) -> String {
    match &event["tool_input"]["command"] {
        Value::String(text) => text.clone(),
        Value::Array(words) => words.iter().map(state::py_str).collect::<Vec<_>>().join(" "),
        _ => String::new(),
    }
}

fn user_prompt_submit(ctx: &Ctx, event: &Value) -> Verdict {
    let prompt = event["prompt"].as_str().unwrap_or("");
    if prompt.trim_start().starts_with("<codex_internal_context") {
        return Verdict::Allow;
    }
    state::with_state(ctx, &state::agent_key(event), |state| {
        let mut prompts = state["prompts"].as_array().cloned().unwrap_or_default();
        prompts.push(prompt.into());
        let keep = prompts.len().saturating_sub(20);
        state.insert("prompts".into(), Value::Array(prompts.split_off(keep)));
    });
    let mut lines = Vec::new();
    if CORRECTION.is_match(prompt) {
        lines.push(
            "Tom is correcting you. Drop exactly what he named and keep going. Don't answer with a new review, verifier, audit, rule, policy or skill edit. If he asked a question, answer it in your first line.",
        );
    }
    if CLOSE_REQUEST.is_match(prompt) {
        lines.push(
            "Tom wants this closed. Read the issue's unchecked Done-when box and run that check yourself in this turn; don't assign it or describe a plan. If it needs a quiet machine, take an `exclusive` capacity lease. Then close the issue, or report the measured miss and fix it now.",
        );
    }
    if lines.is_empty() { Verdict::Allow } else { Verdict::Advise(lines.join(" ")) }
}

pub fn evaluate(input: &HookInput) -> Verdict {
    decide(&Ctx::from_env(), input)
}

pub(crate) fn decide(ctx: &Ctx, input: &HookInput) -> Verdict {
    let Ok(event) = serde_json::from_slice::<Value>(&input.raw) else {
        return Verdict::Allow;
    };
    let tool = event["tool_name"].as_str().unwrap_or("");
    let spawn = tool.ends_with("spawn_agent");
    match event["hook_event_name"].as_str().unwrap_or("") {
        "UserPromptSubmit" => user_prompt_submit(ctx, &event),
        "PreToolUse" if tool == "update_plan" => plan::update_plan(ctx, &event),
        "PreToolUse" if spawn => gates::pre_spawn(input),
        "PreToolUse" if tool == "Bash" || tool == "apply_patch" => {
            match gates::joined_words(input, false).or_else(|| gates::pre_scaffold(ctx, &event)) {
                Some(reason) => Verdict::Deny(reason),
                None if tool == "Bash" => bash::pre_bash(ctx, input, &event),
                None => Verdict::Allow,
            }
        }
        "PostToolUse" if tool == "Bash" => post::post_bash(ctx, input, &event),
        "PostToolUse" if tool == "apply_patch" => post::post_patch(ctx, &event),
        "PostToolUse" if spawn => post::post_spawn(ctx, &event),
        "Stop" => stop::stop(ctx, &event),
        _ => Verdict::Allow,
    }
}


#[cfg(test)]
pub(crate) fn sentence_for_tests(text: &str) -> String {
    stop::sentence(text)
}
