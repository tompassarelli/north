use std::ffi::OsString;
use std::path::Path;

use crate::fleet::model::{Account, LaunchRequest};
use crate::fleet::runtime::{CAPACITY_NEED_ENV, DEFAULT_FOREGROUND_CAP_SECS, SESSION_ENV};

use super::accounts::default_home;

const CONVERSATION_ENV: &str = "CLAUDE_CODE_SESSION_ID";
const CONFIG_ENV: &str = "CLAUDE_CONFIG_DIR";

pub(super) fn claude_env(account: &Account, request: &LaunchRequest) -> Vec<(String, OsString)> {
    let cap_ms = (request.foreground_cap_secs.unwrap_or(DEFAULT_FOREGROUND_CAP_SECS) * 1000).to_string();
    let mut env: Vec<(String, OsString)> = (!at_default(account)).then(|| (CONFIG_ENV.into(), account.home.clone().into())).into_iter().collect();
    env.extend([
        ("DISABLE_AUTOUPDATER".into(), "1".into()),
        ("DISABLE_INSTALLATION_CHECKS".into(), "1".into()),
        ("BASH_DEFAULT_TIMEOUT_MS".into(), cap_ms.clone().into()),
        ("BASH_MAX_TIMEOUT_MS".into(), cap_ms.into()),
        (SESSION_ENV.into(), request.name.clone().into()),
    ]);
    env.extend(request.capacity_need.clone().map(|need| (CAPACITY_NEED_ENV.into(), need.into())));
    env
}

pub(super) fn inherited_env(account: &Account) -> Vec<String> {
    let mut inherited = vec!["CLAUDE_CODE_CHILD_SESSION".into()];
    inherited.extend(at_default(account).then(|| CONFIG_ENV.into()));
    inherited
}

pub(super) fn caller_conversation() -> Option<String> {
    std::env::var(CONVERSATION_ENV).ok().filter(|conversation| !conversation.is_empty())
}

fn at_default(account: &Account) -> bool {
    std::env::var_os("HOME").is_some_and(|home| account.home == default_home(Path::new(&home)))
}
