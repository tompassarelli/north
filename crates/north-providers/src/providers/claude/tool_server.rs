use std::path::{Path, PathBuf};

use north_base::platform::entry_points::{EntryPoint, linked_beside};
use serde_json::json;

use crate::error::{NorthError, NorthResult};
use crate::fleet::model::LaunchRequest;

const TOOL_SERVER_FLAG: &str = "--tool-server";
const CALL_MARGIN: std::time::Duration = std::time::Duration::from_secs(300);

/// The north a launch's tool server and shell prefix run: this process's own binary for a session here, so a candidate runtime serves its own verbs, and the promoted link for a session on another machine, which keeps the home layout but not necessarily this runtime.
pub(super) fn launch_north(request: &LaunchRequest) -> PathBuf {
    match request.machine {
        Some(_) => north_base::platform::north_binary::north_binary(),
        None => north_base::platform::north_binary::own_binary(),
    }
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
