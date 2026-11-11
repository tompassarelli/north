use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde_json::json;

const SANDBOX_FLAG: &str = "--worker-sandbox";
const TOOL_SERVER_FLAG: &str = "--tool-server";
const CALL_MARGIN: std::time::Duration = std::time::Duration::from_secs(300);

/// The one executable that Claude Code's shell prefix names, which runs each Bash command, hook and tool server of every session, the proxy and leads included, under `north --worker-sandbox`: the logins hide from all of them and a worker's removals are confined (north#706, north#977). Claude Code 2.1.322 passes the whole prefix as one program path and the command line as its one argument, so a prefix holding the flag beside the program never started a tool server (north#1151).
pub fn sandbox_wrapper(north: &Path, directory: &Path) -> std::io::Result<PathBuf> {
    let path = directory.join("worker-sandbox");
    let script = format!("#!/bin/sh\nexec '{}' {SANDBOX_FLAG} \"$@\"\n", north.display());
    if std::fs::read_to_string(&path).is_ok_and(|current| current == script) {
        return Ok(path);
    }
    std::fs::create_dir_all(directory)?;
    let staged = directory.join(format!(".worker-sandbox.{}", std::process::id()));
    std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o755).open(&staged)?.write_all(script.as_bytes())?;
    std::fs::rename(&staged, &path)?;
    Ok(path)
}

pub(super) fn shell_prefix() -> Result<PathBuf, String> {
    let north = north_base::platform::north_binary::north_binary();
    let directory = std::env::var_os("XDG_RUNTIME_DIR").filter(|runtime| !runtime.is_empty()).map_or_else(std::env::temp_dir, PathBuf::from).join("north-worker-sandbox");
    sandbox_wrapper(&north, &directory).map_err(|error| format!("{}: {error}", directory.display()))
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
