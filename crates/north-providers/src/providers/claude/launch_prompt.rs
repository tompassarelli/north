use std::io::ErrorKind;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{Account, LaunchRequest, Role};

const FLEET_SETTINGS: &str = r#"{"switchModelsOnFlag":true}"#;
const WORKER_TOOLS: &str = "Bash,Read,Edit,Write,Grep,Glob,TaskStop";
const RESEARCH_TOOLS: &str = ",WebSearch,WebFetch";
const POLICY_FILE: &str = "CLAUDE.md";

/// A worker's system prompt is north's text followed by the account profile's policy file, which is the same for every session of the account; its repository's own instruction files come back through the project and local setting sources, and its one settings file is its profile's hook registration because Claude Code keeps only the last `--settings`.
pub(super) fn args(account: &Account, request: &LaunchRequest, instructions: &str) -> NorthResult<Vec<String>> {
    if request.role() != Role::Worker {
        let mut args = vec!["--settings".to_owned(), FLEET_SETTINGS.to_owned()];
        if !instructions.is_empty() {
            args.extend(["--append-system-prompt".to_owned(), instructions.to_owned()]);
        }
        return Ok(args);
    }
    let settings = account.home.join("settings.json").display().to_string();
    let tools = if researches(request) { format!("{WORKER_TOOLS}{RESEARCH_TOOLS}") } else { WORKER_TOOLS.to_owned() };
    let mut args = ["--tools", &tools, "--setting-sources", "project,local", "--settings", &settings].map(str::to_owned).to_vec();
    let policy = match std::fs::read_to_string(account.home.join(POLICY_FILE)) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => return Err(NorthError::State(format!("{}: {error}", account.home.join(POLICY_FILE).display()))),
    };
    let prompt = [instructions, policy.trim_end()].iter().filter(|text| !text.is_empty()).copied().collect::<Vec<_>>().join("\n\n");
    if !prompt.is_empty() {
        args.extend(["--system-prompt".to_owned(), prompt]);
    }
    Ok(args)
}

fn researches(request: &LaunchRequest) -> bool {
    north_pace::router::brief::brief_header(&request.prompt).get("category").is_some_and(|category| category.trim().eq_ignore_ascii_case("research"))
}
