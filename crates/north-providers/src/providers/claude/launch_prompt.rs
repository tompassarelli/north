use std::io::ErrorKind;
use std::path::Path;

use serde_json::{Value, json};

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Account, LaunchRequest, Role};
use crate::fleet::remote;
use crate::fleet::runtime::Seen;

const SHELL_PREFIX_ENV: &str = "CLAUDE_CODE_SHELL_PREFIX";
const WORKER_TOOLS: &str = "Bash,Read,Edit,Write,Grep,Glob,TaskStop";
const RESEARCH_TOOLS: &str = ",WebSearch,WebFetch";
const POLICY_FILE: &str = "CLAUDE.md";

/// A worker's system prompt is north's text followed by the account profile's policy file, which is the same for every session of the account; its repository's own instruction files come back through the project and local setting sources, and its one settings file is its profile's hook registration because Claude Code keeps only the last `--settings`. A settings file's env outranks the process environment and the profile's names a prefix of two words, so the launch's `prefix` goes into that one settings file (north#1172).
pub(super) fn args(account: &Account, request: &LaunchRequest, instructions: &str, prefix: &Path) -> NorthResult<Vec<String>> {
    if request.role() != Role::Worker {
        let mut args = vec!["--settings".to_owned(), with_prefix(json!({"switchModelsOnFlag": true}), prefix).to_string()];
        if !instructions.is_empty() {
            args.extend(["--append-system-prompt".to_owned(), instructions.to_owned()]);
        }
        return Ok(args);
    }
    let settings = worker_settings(account, request, prefix)?;
    let tools = if researches(request) { format!("{WORKER_TOOLS}{RESEARCH_TOOLS}") } else { WORKER_TOOLS.to_owned() };
    let mut args = ["--tools", &tools, "--setting-sources", "project,local", "--settings", &settings].map(str::to_owned).to_vec();
    let policy = where_it_runs(request, &account.home.join(POLICY_FILE))?.unwrap_or_default();
    let prompt = [instructions, policy.trim_end()].iter().filter(|text| !text.is_empty()).copied().collect::<Vec<_>>().join("\n\n");
    if !prompt.is_empty() {
        args.extend(["--system-prompt".to_owned(), prompt]);
    }
    Ok(args)
}

fn worker_settings(account: &Account, request: &LaunchRequest, prefix: &Path) -> NorthResult<String> {
    let path = account.home.join("settings.json");
    let text = where_it_runs(request, &path)?.ok_or_else(|| NorthError::State(format!("{}{}: no such file", path.display(), on(request))))?;
    let profile = serde_json::from_str(&text).map_err(|error| NorthError::State(format!("{}{}: {error}", path.display(), on(request))))?;
    Ok(with_prefix(profile, prefix).to_string())
}

/// The text of the profile file at `path` where the session runs: on its machine through the session login when it is placed on one, here otherwise; None when no such file is there, an error naming the path and the login when it cannot be read.
fn where_it_runs(request: &LaunchRequest, path: &Path) -> NorthResult<Option<String>> {
    let Some(machine) = request.machine.as_ref() else {
        return match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(text)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(NorthError::State(format!("{}: {error}", path.display()))),
        };
    };
    match remote::read_on(machine, path) {
        Seen::Text(text) => Ok(Some(text)),
        Seen::Absent => Ok(None),
        Seen::Unreachable => Err(NorthError::State(format!("{}{}: unreadable there, or the machine did not answer", path.display(), on(request)))),
    }
}

fn on(request: &LaunchRequest) -> String {
    request.machine.as_ref().map(|machine| format!(" on {} as {}", machine.name, machine.session_login())).unwrap_or_default()
}

fn with_prefix(mut settings: Value, prefix: &Path) -> Value {
    settings["env"][SHELL_PREFIX_ENV] = json!(prefix.display().to_string());
    settings
}

fn researches(request: &LaunchRequest) -> bool {
    north_pace::router::brief::brief_header(&request.prompt).get("category").is_some_and(|category| category.trim().eq_ignore_ascii_case("research"))
}
