use std::ffi::OsString;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Account, LaunchRequest, Role};
use crate::fleet::runtime::{ACTOR_ENV, CAPACITY_NEED_ENV, DEFAULT_FOREGROUND_CAP_SECS, HAND_IN_CAP_SECS, Invocation, ROLE_ENV, SESSION_ENV, Seen};
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

pub(super) fn claude_env(account: &Account, request: &LaunchRequest) -> NorthResult<Vec<(String, OsString)>> {
    let cap_secs = request.foreground_cap_secs.unwrap_or(DEFAULT_FOREGROUND_CAP_SECS);
    let cap_ms = (cap_secs * 1000).to_string();
    let hand_in_ms = (cap_secs.max(HAND_IN_CAP_SECS) * 1000).to_string();
    let mut env = account_env(account);
    env.extend([
        ("DISABLE_AUTOUPDATER".into(), "1".into()),
        ("DISABLE_INSTALLATION_CHECKS".into(), "1".into()),
        ("BASH_DEFAULT_TIMEOUT_MS".into(), cap_ms.into()),
        ("BASH_MAX_TIMEOUT_MS".into(), hand_in_ms.into()),
        (SESSION_ENV.into(), request.name.clone().into()),
        (ROLE_ENV.into(), request.role().label().into()),
        (ACTOR_ENV.into(), request.name.clone().into()),
    ]);
    env.extend(request.capacity_need.clone().map(|need| (CAPACITY_NEED_ENV.into(), need.into())));
    env.extend(request.window().map(|window| (COMPACT_WINDOW_ENV.into(), window.to_string().into())));
    env.extend(git_credential::session_env().into_iter().map(|(name, value)| (name, value.into())));
    env.push((SHELL_PREFIX_ENV.into(), super::tool_server::shell_prefix().map_err(NorthError::State)?.into()));
    Ok(env)
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

/// Why a session on `account` would start without the harness hooks: its config directory, as `read` sees it where the session runs, loads no `settings.json` or one that registers no hook.
pub(super) fn unguarded(account: &Account, read: &dyn Fn(&Path) -> Seen) -> Option<String> {
    if at_default(account) {
        return None;
    }
    let settings = account.home.join("settings.json");
    let why = match read(&settings) {
        Seen::Unreachable => return None,
        Seen::Absent => "has no settings.json",
        Seen::Text(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(data) if data["hooks"].as_object().is_some_and(|hooks| !hooks.is_empty()) => return None,
            Ok(_) => "has a settings.json that registers no hooks",
            Err(_) => "has a settings.json that is not JSON",
        },
    };
    Some(format!("{} {why}, so a session there would run with no hooks; run `north config agents sync`", account.home.display()))
}

/// Why `claude --resume` on `account` in `cwd` would stop at a prompt before the conversation shows: the folder is not trusted, or the account never accepted bypass mode. Run after [`onboard`], which completes the first-run answers north can write.
pub(super) fn interactive_prompt(account: &Account, cwd: &Path) -> Option<String> {
    let config = config_path(account);
    let projects = config.and_then(read).map(|state| state["projects"].clone()).unwrap_or_default();
    if !cwd.ancestors().any(|folder| projects[folder.to_string_lossy().as_ref()]["hasTrustDialogAccepted"] == true) {
        return Some(format!("account profile {} has not trusted {}, so its window would stop at a trust prompt", account.home.display(), cwd.display()));
    }
    let skips_bypass_prompt = read(account.home.join("settings.json")).is_some_and(|settings| settings["skipDangerousModePermissionPrompt"] == true);
    (!skips_bypass_prompt).then(|| format!("account profile {} would ask to confirm bypass permissions in its window", account.home.display()))
}

/// Where `account`'s `.claude.json` lives: beside the default `~/.claude`, inside every other profile.
fn config_path(account: &Account) -> Option<PathBuf> {
    if at_default(account) { account.home.parent().map(|parent| parent.join(".claude.json")) } else { Some(account.home.join(".claude.json")) }
}

fn read(path: PathBuf) -> Option<Value> {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
}

const ONBOARDED: &str = "hasCompletedOnboarding";
const THEME: &str = "dark";
/// Claude Code remembers the last 20 characters of an approved custom API key, in `customApiKeyResponses.approved`.
const KEY_SUFFIX: usize = 20;

/// Writes the first-run answers an interactive launch would otherwise stop for: `hasCompletedOnboarding`, a theme, and, on a pooled API account, the key the launch injects among the approved custom API keys. Writes only when it has to, so a profile Tom has already answered keeps his answers.
pub(super) fn onboard(account: &Account) -> Result<(), String> {
    let path = config_path(account).ok_or_else(|| format!("account profile {} has no parent directory", account.home.display()))?;
    let mut config: Value = match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?,
        Err(error) if error.kind() == ErrorKind::NotFound => json!({}),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let root = config.as_object_mut().ok_or_else(|| format!("{} is not a JSON object", path.display()))?;
    let mut changed = false;
    if root.get(ONBOARDED) != Some(&Value::Bool(true)) {
        root.insert(ONBOARDED.into(), Value::Bool(true));
        changed = true;
    }
    if root.get("theme").and_then(Value::as_str).is_none_or(str::is_empty) {
        root.insert("theme".into(), THEME.into());
        changed = true;
    }
    if let Some(suffix) = approved_key_suffix(account)? {
        let responses = root.entry("customApiKeyResponses").or_insert_with(|| json!({}));
        let responses = responses.as_object_mut().ok_or_else(|| format!("{} customApiKeyResponses is not an object", path.display()))?;
        let approved = responses.entry("approved").or_insert_with(|| json!([]));
        let approved = approved.as_array_mut().ok_or_else(|| format!("{} customApiKeyResponses.approved is not an array", path.display()))?;
        if !approved.iter().any(|value| value.as_str() == Some(suffix.as_str())) {
            approved.push(Value::String(suffix.clone()));
            changed = true;
        }
        if let Some(rejected) = responses.get_mut("rejected").and_then(Value::as_array_mut) {
            let before = rejected.len();
            rejected.retain(|value| value.as_str() != Some(suffix.as_str()));
            changed |= rejected.len() != before;
        }
    }
    if changed { write(path, &config)?; }
    Ok(())
}

/// The suffix Claude Code asks about when `account` launches on a pooled API key, read from the key file the launch itself will read.
fn approved_key_suffix(account: &Account) -> Result<Option<String>, String> {
    let Some(key) = &account.api else { return Ok(None) };
    let text = std::fs::read_to_string(&key.file).map_err(|error| format!("{}: cannot read the API key to approve it: {error}", key.file.display()))?;
    let text = text.trim();
    if text.is_empty() {
        return Err(format!("{}: the API key to approve is empty", key.file.display()));
    }
    let suffix: String = text.chars().skip(text.chars().count().saturating_sub(KEY_SUFFIX)).collect();
    Ok(Some(suffix))
}

/// Replaces `path` with `config` in one rename, so a running client never reads half a file.
fn write(path: PathBuf, config: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(config).map_err(|error| error.to_string())?;
    let staged = path.with_extension(format!("json.north-{}", std::process::id()));
    let written = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&staged).and_then(|mut file| file.write_all(&bytes).and_then(|()| file.sync_all())).and_then(|()| std::fs::rename(&staged, &path));
    written.map_err(|error| {
        let _ = std::fs::remove_file(&staged);
        format!("{}: {error}", path.display())
    })
}

fn at_default(account: &Account) -> bool {
    std::env::var_os("HOME").is_some_and(|home| account.home == default_home(Path::new(&home)))
}
