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

fn claude_once(words: &[String], model: &str, effort: &str, limit: Duration, input: &str) -> Reply {
    let mut args: Vec<&str> = EXTERNAL_BILLING.iter().flat_map(|name| ["-u", *name]).chain(words.iter().map(String::as_str)).collect();
    args.extend([
        "claude", "-p", "--model", model, "--effort", effort, "--tools", "",
        "--strict-mcp-config", "--disable-slash-commands", "--no-session-persistence",
    ]);
    match process::run_with_input_within("env", &args, Some(input.as_bytes()), limit) {
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

/// Asks `model` once on the subscription accounts, trying each profile in turn; an API key in the environment is unset so the call can never bill by token.
pub(crate) fn ask_claude(model: &str, effort: &str, limit: Duration, input: &str, access: &RuntimeAccess, warn: &mut dyn FnMut(String)) -> Asked {
    let profiles = env_or("NORTH_PROFILES", || home().join(".local/state/north/profiles"));
    let accounts = crate::fleet::registry::roster(&access.runtimes).map(|roster| roster.accounts).unwrap_or_default();
    ask(&profiles, &claude_accounts(), &mut |dir| match dir.map_or_else(ambient_billing_refusal, |_| None) {
        Some(why) => Reply { code: Some(1), stdout: String::new(), stderr: why },
        None => claude_once(&account_words(dir, &accounts, access.invocation), model, effort, limit, input),
    }, warn)
}
