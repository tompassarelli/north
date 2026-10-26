use std::ffi::OsString;

use crate::fleet::model::{Account, LaunchRequest};
use crate::fleet::runtime::{CAPACITY_NEED_ENV, DEFAULT_FOREGROUND_CAP_SECS, SESSION_ENV};

const CONVERSATION_ENV: &str = "CLAUDE_CODE_SESSION_ID";

pub(super) fn claude_env(account: &Account, request: &LaunchRequest) -> Vec<(String, OsString)> {
    let cap_ms = (request.foreground_cap_secs.unwrap_or(DEFAULT_FOREGROUND_CAP_SECS) * 1000).to_string();
    let mut env = vec![
        ("CLAUDE_CONFIG_DIR".into(), account.home.clone().into()),
        ("DISABLE_AUTOUPDATER".into(), "1".into()),
        ("DISABLE_INSTALLATION_CHECKS".into(), "1".into()),
        ("BASH_DEFAULT_TIMEOUT_MS".into(), cap_ms.clone().into()),
        ("BASH_MAX_TIMEOUT_MS".into(), cap_ms.into()),
        (SESSION_ENV.into(), request.name.clone().into()),
    ];
    env.extend(request.capacity_need.clone().map(|need| (CAPACITY_NEED_ENV.into(), need.into())));
    env
}

pub(super) fn inherited_env() -> Vec<String> {
    vec!["CLAUDE_CODE_CHILD_SESSION".into()]
}

pub(super) fn caller_conversation() -> Option<String> {
    std::env::var(CONVERSATION_ENV).ok().filter(|conversation| !conversation.is_empty())
}
