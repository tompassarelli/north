use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use north_base::foundation::shim::{copy_executable, write_script};
use north_base::platform::gh_session::SANDBOX_MARK;
use north_providers::providers::claude::{shell_prefix, tool_server_args};
use north_runtime::worker_sandbox::{self, tool_server};
use serde_json::{Value, json};

const LAUNCHER: &str = "a_worker_tool_server_starts_under_the_sandbox_and_answers_one_verb";

fn stand_in(words: &[String]) {
    if words.first().is_some_and(|word| word == worker_sandbox::FLAG) {
        let returned = worker_sandbox::run(&words[1..], |_, _| None);
        panic!("the sandbox wrapper returned instead of starting its program: {returned:?}");
    }
    let north = north_base::platform::north_binary::north_binary();
    tool_server::serve(&words[1..], &north, std::io::stdin().lock(), std::io::stdout().lock()).unwrap();
    std::process::exit(0);
}

fn exchange(server: &mut std::process::Child, requests: &[Value]) -> Vec<Value> {
    let mut stdin = server.stdin.take().unwrap();
    for request in requests {
        writeln!(stdin, "{request}").unwrap();
    }
    drop(stdin);
    let lines = BufReader::new(server.stdout.take().unwrap()).lines().map(Result::unwrap);
    lines.filter_map(|line| serde_json::from_str::<Value>(&line).ok()).collect()
}

#[test]
fn a_worker_tool_server_starts_under_the_sandbox_and_answers_one_verb() {
    let arguments: Vec<String> = std::env::args().collect();
    if let Some(at) = arguments.iter().position(|word| word == "--") {
        return stand_in(&arguments[at + 1..]);
    }
    let scratch = tempfile::tempdir().unwrap();
    let north = scratch.path().join("north");
    let launcher = scratch.path().join("launcher");
    copy_executable(std::env::current_exe().unwrap(), &launcher);
    let stand_in = format!(
        "case \"${{0##*/}}\" in\nworker-sandbox) exec '{}' {LAUNCHER} --exact --nocapture -- --worker-sandbox \"$@\" ;;\nesac\ncase \"$1\" in\n--tool-server) exec '{}' {LAUNCHER} --exact --nocapture -- \"$@\" ;;\nping) printf 'pong %s' \"${SANDBOX_MARK}\" ;;\nesac\n",
        launcher.display(),
        launcher.display()
    );
    write_script(&north, &stand_in);

    let launch = tool_server_args(&north, &["ping".to_owned()]);
    assert_eq!(launch[0], "--mcp-config");
    let config: Value = serde_json::from_str(&launch[1]).unwrap();
    let entry = &config["mcpServers"]["north"];
    let words: Vec<&str> = std::iter::once(entry["command"].as_str().unwrap()).chain(entry["args"].as_array().unwrap().iter().map(|word| word.as_str().unwrap())).collect();
    let prefix = shell_prefix(&north).expect("a launch links worker-sandbox beside its north");
    let mut server = Command::new(&prefix).arg(words.join(" ")).env("NORTH_BIN", &north).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("the shell prefix is one program path that exists");

    let answers = exchange(
        &mut server,
        &[
            json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}}),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "ping", "arguments": {"arguments": []}}}),
        ],
    );
    assert!(server.wait().unwrap().success());

    let listed: Vec<&str> = answers[1]["result"]["tools"].as_array().unwrap().iter().map(|tool| tool["name"].as_str().unwrap()).collect();
    assert_eq!(listed, ["ping"], "{answers:?}");
    assert_eq!(answers[2]["result"]["content"][0]["text"], "pong 1", "the verb ran inside the sandbox wrapper: {answers:?}");
    assert_eq!(answers[2]["result"]["isError"], false);
}
