use std::ffi::OsString;
use std::path::Path;

use crate::fleet::model::{Account, LaunchRequest};
use crate::fleet::runtime::{ACTOR_ENV, CAPACITY_NEED_ENV, DEFAULT_FOREGROUND_CAP_SECS, Invocation, SESSION_ENV};

use super::accounts::default_home;

const CONVERSATION_ENV: &str = "CLAUDE_CODE_SESSION_ID";
const CONFIG_ENV: &str = "CLAUDE_CONFIG_DIR";
const TOKEN_ENV: &str = "CLAUDE_CODE_OAUTH_TOKEN";
const READ_TOKEN: &str = r#"CLAUDE_CODE_OAUTH_TOKEN=$(cat -- "$0") || exit; export CLAUDE_CODE_OAUTH_TOKEN; exec "$@""#;

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
        (ACTOR_ENV.into(), request.name.clone().into()),
    ]);
    env.extend(request.capacity_need.clone().map(|need| (CAPACITY_NEED_ENV.into(), need.into())));
    env
}

pub(super) fn model_args(request: &LaunchRequest) -> Vec<String> {
    let model = request.model.iter().flat_map(|model| ["--model".to_owned(), model.clone()]);
    model.chain(request.effort.iter().flat_map(|effort| ["--effort".to_owned(), effort.label().to_owned()])).collect()
}

pub(super) fn inherited_env(account: &Account) -> Vec<String> {
    let mut inherited = vec!["CLAUDE_CODE_CHILD_SESSION".into(), TOKEN_ENV.into()];
    inherited.extend(at_default(account).then(|| CONFIG_ENV.into()));
    inherited
}

pub(super) fn with_credential(account: &Account, argv: Vec<String>) -> Vec<String> {
    match &account.token_file {
        Some(file) => ["/bin/sh".into(), "-c".into(), READ_TOKEN.into(), file.display().to_string()].into_iter().chain(argv).collect(),
        None => argv,
    }
}

pub(super) fn caller_conversation() -> Option<String> {
    std::env::var(CONVERSATION_ENV).ok().filter(|conversation| !conversation.is_empty())
}

fn at_default(account: &Account) -> bool {
    std::env::var_os("HOME").is_some_and(|home| account.home == default_home(Path::new(&home)))
}
