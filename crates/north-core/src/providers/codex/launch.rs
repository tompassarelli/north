use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{NorthError, NorthResult};
use crate::fleet::runtime::binary;
use crate::providers::install;

const AGENT_CONFIG: [&str; 7] = [
    r#"cli_auth_credentials_store="file""#,
    r#"mcp_oauth_credentials_store="file""#,
    r#"approval_policy="never""#,
    r#"sandbox_mode="danger-full-access""#,
    r#"default_permissions=":danger-full-access""#,
    r#"service_tier="default""#,
    "agents.max_concurrent_threads_per_session=64",
];
const CONTEXT_FILE: &str = "/etc/north/agent-context.json";
const CONTEXT_KEYS: [(&str, &str); 2] = [("model_context_window", "window"), ("model_auto_compact_token_limit", "autoCompactTokenLimit")];
const SUBAGENT_DEFAULT: [&str; 2] = ["gpt-6.1-sol", "high"];
const APP_SERVER: [&str; 7] = ["app-server", "--listen", "stdio://", "--enable", "multi_agent_v2", "--enable", "default_mode_request_user_input"];

pub struct AppServer {
    pub argv: Vec<String>,
    pub env: Vec<(String, OsString)>,
}

pub fn session_server(account_home: &Path) -> NorthResult<AppServer> {
    server(account_home, Vec::new())
}

fn server(account_home: &Path, mut argv: Vec<String>) -> NorthResult<AppServer> {
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    argv.extend(masked(&home, std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from)));
    argv.push(install::runtime("codex").display().to_string());
    for setting in settings(std::fs::read_to_string(CONTEXT_FILE).ok().as_deref(), SUBAGENT_DEFAULT.map(str::to_owned))? {
        argv.extend(["-c".to_owned(), setting]);
    }
    argv.extend(APP_SERVER.map(str::to_owned));
    Ok(AppServer { argv, env: account_env(account_home) })
}

fn account_env(home: &Path) -> Vec<(String, OsString)> {
    let mut env = vec![("CODEX_HOME".into(), home.as_os_str().to_owned())];
    let sqlite = home.join("sqlite");
    if sqlite.is_dir() {
        env.push(("CODEX_SQLITE_HOME".into(), sqlite.into_os_string()));
    }
    env
}

fn masked(home: &Path, claude_config: Option<PathBuf>) -> Vec<String> {
    let mut argv = vec![binary("bwrap").display().to_string(), "--dev-bind".into(), "/".into(), "/".into()];
    for sessions in std::iter::once(home.join(".claude")).chain(claude_config).map(|dir| dir.join("sessions")) {
        argv.extend(["--tmpfs".into(), sessions.display().to_string()]);
    }
    argv.push("--".into());
    argv
}

fn settings(context: Option<&str>, [model, effort]: [String; 2]) -> NorthResult<Vec<String>> {
    let mut settings: Vec<String> = AGENT_CONFIG.map(str::to_owned).into();
    settings.extend(context_settings(context)?);
    settings.push(format!(r#"agents.default_subagent_model="{model}""#));
    settings.push(format!(r#"agents.default_subagent_reasoning_effort="{effort}""#));
    Ok(settings)
}

fn context_settings(context: Option<&str>) -> NorthResult<Vec<String>> {
    let Some(text) = context else { return Ok(Vec::new()) };
    let declared: Value = serde_json::from_str(text).unwrap_or_default();
    let limits: Option<Vec<String>> = CONTEXT_KEYS
        .iter()
        .map(|(key, field)| declared[field].as_u64().filter(|limit| *limit > 0).map(|limit| format!("{key}={limit}")))
        .collect();
    limits.ok_or_else(|| NorthError::State(format!("{CONTEXT_FILE} declares no valid context limits")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_app_server_runs_with_full_access_and_the_declared_context() {
        let settings = settings(Some(r#"{"window": 500000, "autoCompactTokenLimit": 467000}"#), ["m".into(), "e".into()]).unwrap();
        for expected in [r#"sandbox_mode="danger-full-access""#, r#"approval_policy="never""#, r#"cli_auth_credentials_store="file""#, "model_context_window=500000", "model_auto_compact_token_limit=467000", r#"agents.default_subagent_model="m""#] {
            assert!(settings.iter().any(|setting| setting == expected), "{expected} missing from {settings:?}");
        }
        for invalid in [r#"{"window": 0, "autoCompactTokenLimit": 1}"#, "not json"] {
            assert!(context_settings(Some(invalid)).is_err(), "an invalid declaration must refuse the launch: {invalid}");
        }
        assert!(context_settings(None).unwrap().is_empty());
    }

    #[test]
    fn claude_session_files_are_masked_from_codex() {
        let argv = masked(Path::new("/home/u"), Some(PathBuf::from("/state/claude-a")));
        let masks: Vec<&str> = argv.windows(2).filter(|pair| pair[0] == "--tmpfs").map(|pair| pair[1].as_str()).collect();
        assert_eq!(masks, ["/home/u/.claude/sessions", "/state/claude-a/sessions"]);
        assert_eq!(argv.last().map(String::as_str), Some("--"));
    }
}
