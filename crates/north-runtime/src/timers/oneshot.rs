use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::{env_or, home, north_program, text};
use crate::fleet::model::Account;
use crate::platform::process;
use crate::fleet::runtime::Invocation;
use crate::runtime_access::RuntimeAccess;

/// Environment that routes `claude` to billing outside the subscription; a one-shot ask unsets all of it.
pub(crate) const EXTERNAL_BILLING: [&str; 6] = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL", "CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CODE_USE_VERTEX", "CLAUDE_CODE_USE_FOUNDRY"];
pub(crate) const QUIET: &str = "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC";
const QUIET_WORD: &str = "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1";


pub(crate) struct Reply {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) enum Asked {
    Answer(String),
    AtLimit,
    Failed(String),
}

fn at_limit(text: &str) -> bool {
    text.find("hit your").is_some_and(|start| text[start..].contains("limit"))
}

pub(crate) fn ask(profiles: &Path, labels: &[String], attempt: &mut dyn FnMut(Option<&Path>) -> Reply, warn: &mut dyn FnMut(String)) -> Asked {
    let (mut tried, mut failed) = (false, false);
    for label in labels {
        let dir = profiles.join(format!("claude-{label}"));
        if !dir.is_dir() {
            continue;
        }
        tried = true;
        let reply = attempt(Some(&dir));
        if reply.code == Some(0) && !reply.stdout.is_empty() {
            return Asked::Answer(reply.stdout);
        }
        let combined = format!("{}{}", reply.stdout, reply.stderr);
        if !at_limit(&combined) {
            failed = true;
            warn(format!("claude account {label} failed: {combined}"));
        }
    }
    if !tried {
        let reply = attempt(None);
        return if reply.code == Some(0) { Asked::Answer(reply.stdout) } else { Asked::Failed(super::child_failure("claude", reply.code, reply.stderr.as_bytes())) };
    }
    if failed { Asked::Failed("a claude account failed".to_owned()) } else { Asked::AtLimit }
}

fn claude_accounts() -> Vec<String> {
    let listed = Command::new(north_program()).args(["fleet", "accounts"]).output().map(|output| text(&output)).unwrap_or_default();
    listed.lines().filter_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next() == Some("claude")).then(|| fields.next().map(str::to_owned)).flatten()
    }).collect()
}

/// The `env` words that start `claude` as the profile at `config_dir`: through its account's
/// invocation when the roster lists one there, since a token-file account is not logged in otherwise.
pub(crate) fn account_words(config_dir: Option<&Path>, accounts: &[Account], invocation_of: fn(&Account) -> Invocation) -> Vec<String> {
    let fast = "CLAUDE_CODE_DISABLE_FAST_MODE=1".to_owned();
    let Some(dir) = config_dir else { return vec![fast] };
    let Some(account) = accounts.iter().find(|account| account.home == dir) else { return vec![format!("CLAUDE_CONFIG_DIR={}", dir.display()), fast] };
    let invocation = invocation_of(account);
    let unset = invocation.env_remove.iter().flat_map(|name| ["-u".to_owned(), name.clone()]);
    let set = invocation.env.iter().map(|(name, value)| format!("{name}={}", value.to_string_lossy()));
    unset.chain(set).chain([fast]).chain(invocation.prefix).collect()
}

pub struct Ask<'a> {
    pub model: &'a str,
    pub effort: &'a str,
    pub system: &'a str,
    pub input: &'a str,
}

pub(crate) fn lean_flags<'a>(ask: &Ask<'a>) -> [&'a str; 14] {
    ["-p", "--model", ask.model, "--effort", ask.effort, "--system-prompt", ask.system, "--tools", "", "--setting-sources", "", "--strict-mcp-config", "--disable-slash-commands", "--no-session-persistence"]
}

fn claude_once(words: &[String], ask: &Ask, extra: &[&str], limit: Duration) -> Reply {
    let mut args: Vec<&str> = EXTERNAL_BILLING.iter().flat_map(|name| ["-u", *name]).chain([QUIET_WORD]).chain(words.iter().map(String::as_str)).collect();
    args.push("claude");
    args.extend(lean_flags(ask));
    args.extend(extra);
    match process::run_with_input_within("env", &args, Some(ask.input.as_bytes()), limit) {
        Some(output) => Reply { code: output.status.code(), stdout: text(&output), stderr: String::from_utf8_lossy(&output.stderr).into_owned() },
        None => Reply { code: None, stdout: String::new(), stderr: "claude did not finish".to_owned() },
    }
}

/// Why the default login (used when no `claude-*` profile exists) would bill by token instead of the subscription, if it would.
fn ambient_billing_refusal() -> Option<String> {
    if let Some(name) = ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"].into_iter().find(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty())) {
        return Some(format!("{name} is set, so the default login would bill by token"));
    }
    let dir = env_or("CLAUDE_CONFIG_DIR", || home().join(".claude"));
    let read = |file: &str| std::fs::read_to_string(dir.join(file)).ok().and_then(|held| serde_json::from_str::<serde_json::Value>(&held).ok());
    if ["settings.json", "settings.local.json"].iter().any(|file| read(file).is_some_and(|settings| settings.get("apiKeyHelper").is_some())) {
        return Some("an apiKeyHelper is configured, so the default login would bill by token".to_owned());
    }
    read(".credentials.json").is_none_or(|credentials| credentials.get("claudeAiOauth").is_none()).then(|| "the default login is not a claude.ai subscription login".to_owned())
}

fn answered_record() -> std::path::PathBuf {
    home().join(".local/state/north/oneshot-answered")
}

fn answered_first(mut labels: Vec<String>, reserve: &[String], answered: Option<&str>) -> Vec<String> {
    if let Some(at) = answered.and_then(|answered| labels.iter().position(|label| label == answered)) {
        let label = labels.remove(at);
        let front = if reserve.contains(&label) { 0 } else { labels.iter().take_while(|held| reserve.contains(held)).count() };
        labels.insert(front, label);
    }
    labels
}

fn remember_answered(dir: &Path) {
    if let Some(label) = dir.file_name().and_then(|name| name.to_str()).and_then(|name| name.strip_prefix("claude-")) {
        let _ = std::fs::write(answered_record(), label);
    }
}

/// Asks once on the subscription accounts, trying each profile in turn, the last one to answer first; an API key in the environment is unset and the api-key account is skipped, so the call can never bill by token.
pub(crate) fn ask_claude(question: &Ask, limit: Duration, access: &RuntimeAccess, warn: &mut dyn FnMut(String)) -> Asked {
    ask_claude_with(question, &[], limit, access, warn)
}

fn ask_claude_with(question: &Ask, extra: &[&str], limit: Duration, access: &RuntimeAccess, warn: &mut dyn FnMut(String)) -> Asked {
    let profiles = env_or("NORTH_PROFILES", || home().join(".local/state/north/profiles"));
    let accounts = crate::fleet::registry::roster(&access.runtimes).map(|roster| roster.accounts).unwrap_or_default();
    let shared: Vec<String> = claude_accounts().into_iter().filter(|label| !accounts.iter().any(|account| &account.label == label && account.api.is_some())).collect();
    let reserve = crate::platform::orchestration::read().map(|policy| crate::spend::operational::declared(&policy)).unwrap_or_default();
    let answered = std::fs::read_to_string(answered_record()).ok();
    let subscription = answered_first(crate::spend::operational::order(&reserve, shared), &reserve, answered.as_deref().map(str::trim));
    ask(&profiles, &subscription, &mut |dir| match dir.map_or_else(ambient_billing_refusal, |_| None) {
        Some(why) => Reply { code: Some(1), stdout: String::new(), stderr: why },
        None => {
            let reply = claude_once(&account_words(dir, &accounts, access.invocation), question, extra, limit);
            if let Some(dir) = dir.filter(|_| reply.code == Some(0) && !reply.stdout.is_empty()) {
                remember_answered(dir);
            }
            reply
        }
    }, warn)
}

pub struct Measured {
    pub text: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

pub fn ask_claude_measured(question: &Ask, limit: Duration, access: &RuntimeAccess, warn: &mut dyn FnMut(String)) -> Result<Measured, String> {
    let answer = match ask_claude_with(question, &["--output-format", "json"], limit, access, warn) {
        Asked::Answer(answer) => answer,
        Asked::AtLimit => return Err("every claude subscription account is at its usage limit".to_owned()),
        Asked::Failed(reason) => return Err(reason),
    };
    let result: serde_json::Value = serde_json::from_str(answer.trim()).map_err(|why| format!("claude printed no JSON result: {why}"))?;
    let text = result.get("result").and_then(serde_json::Value::as_str).ok_or("claude's JSON result holds no result text")?.to_owned();
    let count = |key: &str| result.get("usage").and_then(|usage| usage.get(key)).and_then(serde_json::Value::as_u64).unwrap_or(0);
    Ok(Measured { text, input_tokens: count("input_tokens"), output_tokens: count("output_tokens"), cache_read_tokens: count("cache_read_input_tokens"), cache_write_tokens: count("cache_creation_input_tokens") })
}
