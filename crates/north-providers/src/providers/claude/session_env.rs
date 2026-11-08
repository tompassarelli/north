use std::ffi::OsString;
use std::path::Path;

use crate::fleet::model::{Account, LaunchRequest, Role};
use crate::fleet::runtime::{ACTOR_ENV, CAPACITY_NEED_ENV, DEFAULT_FOREGROUND_CAP_SECS, Invocation, ROLE_ENV, SESSION_ENV};
use crate::platform::git_credential;

use super::accounts::default_home;

const CONVERSATION_ENV: &str = "CLAUDE_CODE_SESSION_ID";
const CONFIG_ENV: &str = "CLAUDE_CONFIG_DIR";
const TOKEN_ENV: &str = "CLAUDE_CODE_OAUTH_TOKEN";
const COMPACT_WINDOW_ENV: &str = "CLAUDE_CODE_AUTO_COMPACT_WINDOW";
const READ_TOKEN: &str = r#"CLAUDE_CODE_OAUTH_TOKEN=$(cat -- "$0") || exit; export CLAUDE_CODE_OAUTH_TOKEN; exec "$@""#;
const READ_API_KEY: &str = r#"ANTHROPIC_API_KEY=$(cat -- "$0") || exit; export ANTHROPIC_API_KEY; exec "$@""#;
const API_KEY_ENV: &str = "ANTHROPIC_API_KEY";
const SHELL_PREFIX_ENV: &str = "CLAUDE_CODE_SHELL_PREFIX";

fn account_env(account: &Account) -> Vec<(String, OsString)> {
    (!at_default(account)).then(|| (CONFIG_ENV.into(), account.home.clone().into())).into_iter().collect()
}

pub fn invocation(account: &Account) -> Invocation {
    Invocation { prefix: with_credential(account, Vec::new()), env: account_env(account), env_remove: inherited_env(account) }
}

pub(super) fn claude_env(account: &Account, request: &LaunchRequest) -> Vec<(String, OsString)> {
    let cap_ms = (request.foreground_cap_secs.unwrap_or(DEFAULT_FOREGROUND_CAP_SECS) * 1000).to_string();
    let mut env = account_env(account);
    env.extend([
        ("DISABLE_AUTOUPDATER".into(), "1".into()),
        ("DISABLE_INSTALLATION_CHECKS".into(), "1".into()),
        ("BASH_DEFAULT_TIMEOUT_MS".into(), cap_ms.clone().into()),
        ("BASH_MAX_TIMEOUT_MS".into(), cap_ms.into()),
        (SESSION_ENV.into(), request.name.clone().into()),
        (ROLE_ENV.into(), request.role().label().into()),
        (ACTOR_ENV.into(), request.name.clone().into()),
    ]);
    env.extend(request.capacity_need.clone().map(|need| (CAPACITY_NEED_ENV.into(), need.into())));
    env.extend(request.window().map(|window| (COMPACT_WINDOW_ENV.into(), window.to_string().into())));
    env.extend(git_credential::session_env().into_iter().map(|(name, value)| (name, value.into())));
    env.extend(worker_sandbox(request));
    env
}

/// The shell prefix that runs each Bash command, hook and MCP server of a worker under `north --worker-sandbox`, once the `worker-sandbox` gate of harness/gates.toml enforces, or for a canary launch made with NORTH_WORKER_SANDBOX=1 while it is in shadow (north#706).
fn worker_sandbox(request: &LaunchRequest) -> Option<(String, OsString)> {
    let applies = north_base::platform::harness::document("harness/gates.toml")["gate"]["worker-sandbox"]["mode"].as_str() == Some("enforce") || std::env::var_os("NORTH_WORKER_SANDBOX").is_some_and(|canary| canary == "1");
    (applies && request.role() == Role::Worker).then(|| (SHELL_PREFIX_ENV.into(), format!("{} --worker-sandbox", north_base::platform::north_binary::north_binary().display()).into()))
}

pub(super) fn model_args(request: &LaunchRequest) -> Vec<String> {
    let model = request.model.iter().flat_map(|model| ["--model".to_owned(), model.clone()]);
    model.chain(request.effort.iter().flat_map(|effort| ["--effort".to_owned(), effort.label().to_owned()])).collect()
}

pub(super) fn agent_args(request: &LaunchRequest) -> Vec<String> {
    match request.role() {
        Role::Proxy => vec!["--agent".to_owned(), Role::Proxy.label().to_owned()],
        Role::Lead | Role::Worker => Vec::new(),
    }
}

pub(super) fn inherited_env(account: &Account) -> Vec<String> {
    let mut inherited = vec!["CLAUDE_CODE_CHILD_SESSION".into(), TOKEN_ENV.into()];
    inherited.extend(account.api.is_none().then(|| API_KEY_ENV.into()));
    inherited.extend(at_default(account).then(|| CONFIG_ENV.into()));
    inherited
}

pub(super) fn with_credential(account: &Account, argv: Vec<String>) -> Vec<String> {
    let wrapped = |script: &str, file: &Path| ["/bin/sh".into(), "-c".into(), script.into(), file.display().to_string()].into_iter().chain(argv.clone()).collect();
    match (&account.token_file, &account.api) {
        (Some(file), _) => wrapped(READ_TOKEN, file),
        (None, Some(key)) => wrapped(READ_API_KEY, &key.file),
        (None, None) => argv,
    }
}

pub(super) fn caller_conversation() -> Option<String> {
    std::env::var(CONVERSATION_ENV).ok().filter(|conversation| !conversation.is_empty())
}

/// Why a session on `account` would start without the harness hooks: its config directory, as `present` sees it where the session runs, loads no `settings.json`.
pub(super) fn unguarded(account: &Account, present: &dyn Fn(&Path) -> bool) -> Option<String> {
    (!at_default(account) && !present(&account.home.join("settings.json"))).then(|| {
        format!("{} has no settings.json, so a session there would run with no hooks; run `north config agents sync`", account.home.display())
    })
}

/// Why `claude --resume` on `account` in `cwd` with bypassed permissions would stop at a prompt before the conversation shows: the folder is not trusted, or the account never accepted bypass mode.
pub(super) fn interactive_prompt(account: &Account, cwd: &Path) -> Option<String> {
    let read = |path: std::path::PathBuf| std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let config = if at_default(account) { account.home.parent().map(|parent| parent.join(".claude.json")) } else { Some(account.home.join(".claude.json")) };
    let projects = config.and_then(read).map(|state| state["projects"].clone()).unwrap_or_default();
    if !cwd.ancestors().any(|folder| projects[folder.to_string_lossy().as_ref()]["hasTrustDialogAccepted"] == true) {
        return Some(format!("account profile {} has not trusted {}, so its window would stop at a trust prompt", account.home.display(), cwd.display()));
    }
    let skips_bypass_prompt = read(account.home.join("settings.json")).is_some_and(|settings| settings["skipDangerousModePermissionPrompt"] == true);
    (!skips_bypass_prompt).then(|| format!("account profile {} would ask to confirm bypass permissions in its window", account.home.display()))
}

fn at_default(account: &Account) -> bool {
    std::env::var_os("HOME").is_some_and(|home| account.home == default_home(Path::new(&home)))
}
