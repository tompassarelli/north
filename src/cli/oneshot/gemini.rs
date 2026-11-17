//! The Gemini side of `north check oneshot` (north#1650): one headless `agy` print-mode call, the one way north reaches Gemini. It runs as a custom agent listing no tools, in a fresh workspace holding only that agent, with every tool action also denied through session-only settings, a stored API key or application default credentials refusing the start, no permission auto-approved, slash commands off and every API key unset, so the model answers from the prompt alone and the call can never bill an API.
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use north_base::platform::process;

use super::session::Ask;

const FAMILY: &str = "gemini-";
/// Linux refuses one argument longer than this (MAX_ARG_STRLEN), and agy takes the prompt as one argument.
const ARGUMENT_LIMIT: usize = 128 * 1024;
const API_KEYS: [&str; 6] = ["GEMINI_API_KEY", "GOOGLE_API_KEY", "GOOGLE_APPLICATION_CREDENTIALS", "GOOGLE_GENAI_USE_VERTEXAI", "GOOGLE_CLOUD_PROJECT", "CLOUDSDK_CONFIG"];
const ADMIN_SETTINGS: &str = "/etc/antigravity/admin_settings.json";
const BILLED_AUTH: [&str; 4] = ["apikey", "api_key", "gemini-api-key", "vertex"];

/// Whether `model` is a Gemini model, asked through agy.
pub(crate) fn serves(model: &str) -> bool {
    model.starts_with(FAMILY)
}

/// agy's session-only settings denying every action its tools take, so the model can answer but run, read, write, fetch or call nothing.
const DENY_EVERY_TOOL: &str = r#"{"permissions":{"deny":["command(*)","unsandboxed(*)","read_file(*)","write_file(*)","read_url(*)","execute_url(*)","download_url(*)","mcp(*)"]}}"#;

/// The agent the call runs as, written into the empty workspace: a main agent whose `tools` list is empty, so the model is given no tool at all, web search included, which no permission kind covers.
const AGENT_NAME: &str = "north-oneshot";
const AGENT: &str = "---\nname: north-oneshot\ndescription: Answers one question from its prompt alone, with no tools.\nmainAgent: true\nsubagent: false\ntools: []\n---\n# north-oneshot\nAnswer from the prompt alone. You have no tools.\n";

/// The words agy is started with for `question` and `prompt`: the model and effort, the toolless agent, every tool action denied, the terminal sandbox, and slash commands off.
fn arguments<'a>(question: &'a Ask, prompt: &'a str) -> [&'a str; 12] {
    ["--model", question.model, "--effort", question.effort, "--agent", AGENT_NAME, "--settings", DENY_EVERY_TOOL, "--sandbox", "--disable-slash-commands", "-p", prompt]
}

/// Why agy could bill an API from what `home` and the host hold, if it could: agy's user or admin settings naming an API key or Vertex auth, or gcloud's application default credentials, which agy would use with no key in the environment.
fn stored_billed_auth(home: &Path, admin: &Path) -> Option<String> {
    for settings in [home.join(".gemini/antigravity-cli/settings.json"), admin.to_owned()] {
        let text = fs::read_to_string(&settings).unwrap_or_default().to_lowercase();
        if let Some(word) = BILLED_AUTH.into_iter().find(|word| text.contains(word)) {
            return Some(format!("{} names {word}, so agy could bill an API instead of the signed-in plan", settings.display()));
        }
    }
    let credentials = home.join(".config/gcloud/application_default_credentials.json");
    credentials.exists().then(|| format!("{} holds application default credentials, which bill a Google Cloud project", credentials.display()))
}

/// The answer agy gives `question` within `limit`, or why it gave none.
pub(crate) fn ask(question: &Ask, limit: Duration) -> Result<String, String> {
    let prompt = format!("{}\n\n{}", question.system, question.input);
    if prompt.len() >= ARGUMENT_LIMIT {
        return Err(format!("the prompt is {} bytes, over the {ARGUMENT_LIMIT} bytes one argument to agy may hold, so agy is not started", prompt.len()));
    }
    if let Some(why) = stored_billed_auth(&super::home(), Path::new(ADMIN_SETTINGS)) {
        return Err(format!("{why}; agy is not started"));
    }
    let workspace = tempfile::tempdir().map_err(|error| format!("no empty workspace for agy: {error}"))?;
    let agents = workspace.path().join(".agents/agents");
    fs::create_dir_all(&agents).and_then(|()| fs::write(agents.join(format!("{AGENT_NAME}.md")), AGENT)).map_err(|error| format!("no toolless agent for agy in its workspace: {error}"))?;
    let mut command = Command::new(agy_program());
    command.current_dir(workspace.path()).stdin(Stdio::null()).env("AGY_CLI_DISABLE_AUTO_UPDATE", "1").env("BROWSER", "false").env_remove("DISPLAY").env_remove("WAYLAND_DISPLAY");
    for key in API_KEYS {
        command.env_remove(key);
    }
    command.args(arguments(question, &prompt));
    let output = process::try_run_command_within(command, None, limit).map_err(|error| format!("agy did not start: {error}"))?.ok_or_else(|| format!("agy gave no answer within {} seconds", limit.as_secs()))?;
    let answer = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() && !answer.trim().is_empty() {
        Ok(answer)
    } else {
        Err(super::child_failure("agy", output.status.code(), &output.stderr))
    }
}

fn agy_program() -> std::path::PathBuf {
    let chosen = std::env::var_os("AGY_RUNTIME").map(std::path::PathBuf::from);
    let installed = std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(".local/lib/agy/current/bin/agy"));
    chosen.or(installed).filter(|path| path.is_file()).unwrap_or_else(|| "agy".into())
}
