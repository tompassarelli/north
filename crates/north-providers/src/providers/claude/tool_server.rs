use std::path::{Path, PathBuf};

use north_base::platform::agent_runtime;
use north_base::platform::entry_points::{EntryPoint, linked_beside};
use serde_json::json;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::{LaunchRequest, Machine};
use crate::fleet::runtime::Seen;

const TOOL_SERVER_FLAG: &str = "--tool-server";
const CALL_MARGIN: std::time::Duration = std::time::Duration::from_secs(300);

/// The north a launch's tool server runs and the one program its shell prefix names.
pub(super) struct LaunchPrograms {
    pub(super) north: PathBuf,
    pub(super) prefix: PathBuf,
}

/// The programs a launch's tool server and shell prefix run: this process's own binary for a session here, so a candidate runtime serves its own verbs; the promoted link for a session on another machine as the operator, which keeps the home layout but not necessarily this runtime; and for a session as the agent, the runtime the naming file on its machine names, asked through `ask` with the check the agent's login takes it with, so no program under either user's home and no file another user could write chooses the sandbox the session runs in.
pub(super) fn launch_programs(request: &LaunchRequest, operator_home: &Path, ask: impl Fn(&Machine, &str) -> Seen) -> NorthResult<LaunchPrograms> {
    match request.machine.as_ref() {
        Some(machine) if machine.session_home(operator_home).is_some() => agent_programs(machine, ask(machine, &agent_runtime::named_link_printed())),
        Some(_) => beside(north_base::platform::north_binary::north_binary()),
        None => beside(north_base::platform::north_binary::own_binary()),
    }
}

fn beside(north: PathBuf) -> NorthResult<LaunchPrograms> {
    let prefix = shell_prefix(&north)?;
    Ok(LaunchPrograms { north, prefix })
}

/// The agent runtime's own `north` and `worker-sandbox`, which its publish puts side by side in a store path the agent cannot change.
fn agent_programs(machine: &Machine, named: Seen) -> NorthResult<LaunchPrograms> {
    let link = match named {
        Seen::Text(text) => PathBuf::from(text.lines().next().unwrap_or_default().trim()),
        Seen::Absent | Seen::Unreachable => PathBuf::new(),
    };
    if !link.is_absolute() {
        return Err(NorthError::State(format!("{} on {} as {} names no agent runtime in a root-owned file no one else can write, or the machine did not answer, so the session would have none of north's tools", agent_runtime::NAMING_FILE, machine.name, machine.session_login())));
    }
    Ok(LaunchPrograms { north: link.join("north"), prefix: link.join(EntryPoint::WorkerSandbox.name()) })
}

/// The one executable Claude Code's shell prefix names, which runs each Bash command, hook and tool server of a session under `north --worker-sandbox`: the `worker-sandbox` link beside `north`. Claude Code 2.1.322 spawns the whole prefix as one program path with the command line as its one argument, so a prefix holding the flag beside the program started no tool server (north#1151, north#1172). A launch that cannot make the link fails here, because a worker started without it has none of north's tools.
pub fn shell_prefix(north: &Path) -> NorthResult<PathBuf> {
    linked_beside(north, EntryPoint::WorkerSandbox).map_err(|error| NorthError::State(format!("cannot link worker-sandbox beside {}, so the session's tool server could not start: {error}", north.display())))
}

/// The `--mcp-config` argument that starts one tool server offering `verbs`, or nothing for no verbs; the shell prefix puts the server in the sandbox (north#1151). Its call timeout sits above the longest hand-in wait, so a `hand-in` call returns the lander's verdict instead of timing out (north#1158).
pub fn tool_server_args(north: &Path, verbs: &[String]) -> Vec<String> {
    if verbs.is_empty() {
        return Vec::new();
    }
    let args: Vec<&str> = [TOOL_SERVER_FLAG].into_iter().chain(verbs.iter().map(String::as_str)).collect();
    let timeout = (north_base::land_ticket::HAND_IN_WAIT + CALL_MARGIN).as_millis();
    let server = json!({"mcpServers": {"north": {"command": north.display().to_string(), "args": args, "timeout": timeout}}});
    vec!["--mcp-config".to_owned(), server.to_string()]
}

pub(super) fn configured_verbs() -> Vec<String> {
    let verbs = north_base::platform::harness::document("harness/worker-tools.toml")["verbs"].as_array();
    verbs.into_iter().flatten().filter_map(|verb| verb.as_str().map(str::to_owned)).collect()
}
