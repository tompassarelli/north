use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use north_base::foundation::shim::write_executable;
use north_base::platform::wait;
use north_providers::providers::claude::{shell_prefix, tool_server_args};
use north_runtime::worker_sandbox::BROKER_SOCKET_ENV;
use serde_json::{Value, json};

const OLDER_RUNTIME: &str = "4068cfe1d1a0000000000000000000000000000a";
const OLDER_NORTH: &str = "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$OLDER_NORTH_CALLS\"\nprintf \"error: unrecognized command '%s'\\n\\nUsage: north [COMMAND]\\n\" \"$*\" >&2\nexit 2\n";
const CHANGE: &str = "the worker's change\n";
const MESSAGE: &str = "Add the worker's change";
const SERVE_WAIT: Duration = Duration::from_secs(30);
const CALL_WAIT: Duration = Duration::from_secs(240);
const JUDGED_NORTH: [&str; 3] = ["AGENT_CAPACITY_NORTH", "NORTH_BIN", "NORTH_LAND_NORTH"];

fn git(directory: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(directory).args(arguments).env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
    assert!(output.status.success(), "git {arguments:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

struct Canary {
    root: tempfile::TempDir,
    candidate: PathBuf,
    origin: PathBuf,
    main: PathBuf,
    lane: PathBuf,
}

impl Canary {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let path = |name: &str| root.path().join(name);
        let runtimes = path("home/.local/state/north/runtimes");
        let older = runtimes.join(OLDER_RUNTIME).join("north");
        fs::create_dir_all(older.parent().unwrap()).unwrap();
        write_executable(&older, OLDER_NORTH);
        symlink(Path::new("runtimes").join(OLDER_RUNTIME), path("home/.local/state/north/current")).unwrap();
        fs::create_dir_all(path("live-bin")).unwrap();
        symlink(&older, path("live-bin/north")).unwrap();
        let candidate = path("candidate/north");
        fs::create_dir_all(path("candidate")).unwrap();
        symlink(crate::support::bin_exe!("CARGO_BIN_EXE_north"), &candidate).unwrap();
        for directory in ["state", "run"] {
            fs::create_dir_all(path(directory)).unwrap();
        }

        let (origin, main, lane) = (path("project/origin.git"), path("project/main"), path("project/worktrees/worker"));
        fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        for (key, value) in [("user.name", "canary"), ("user.email", "canary@example.invalid"), ("commit.gpgsign", "false")] {
            git(&main, &["config", key, value]);
        }
        fs::write(main.join("README.md"), "The scratch repository the candidate's hand-in tool lands one change into.\n").unwrap();
        let host = north_base::platform::land_queue::local_host().split('.').next().unwrap().to_owned();
        fs::write(main.join(".safe-push"), format!("[landing]\n\tqueue = lander\n\tlander-host = {host}\n\tremote-hosts =\n[land]\n\tcheck = ** :: true\n")).unwrap();
        git(&main, &["add", "."]);
        git(&main, &["commit", "-q", "-m", "Start the scratch repository"]);
        git(root.path(), &["clone", "-q", "--bare", main.to_str().unwrap(), origin.to_str().unwrap()]);
        git(&main, &["remote", "add", "origin", origin.to_str().unwrap()]);
        git(&main, &["fetch", "-q", "origin"]);
        git(&main, &["worktree", "add", "-q", "-b", "worker", lane.to_str().unwrap(), "origin/main"]);
        Self { root, candidate, origin, main, lane }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.path().join(name)
    }

    fn command(&self, program: &Path) -> Command {
        let mut command = Command::new(program);
        let path = format!("{}:{}", self.path("live-bin").display(), std::env::var("PATH").unwrap_or_default());
        command
            .env_clear()
            .env("PATH", path)
            .env("HOME", self.path("home"))
            .env("TMPDIR", std::env::temp_dir())
            .env("XDG_STATE_HOME", self.path("state"))
            .env("XDG_RUNTIME_DIR", self.path("run"))
            .env("NORTH_TEST_ROOT", self.path("state"))
            .env("NORTH_FLEET_TEST", "1")
            .env("THREADS_DB", self.path("state/threads.db"))
            .env("NORTH_PUSH_LOG", self.path("pushes.tsv"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("OLDER_NORTH_CALLS", self.path("older.calls"))
            .env(BROKER_SOCKET_ENV, self.path("broker.sock"))
            .stdin(Stdio::null());
        command
    }

    fn start(&self, arguments: &[&str], pins: &[&str], log: &str) -> Running {
        let log = fs::File::create(self.path(log)).unwrap();
        let child = self.command(&self.candidate).args(arguments).envs(pins.iter().map(|pin| (pin, &self.candidate))).stdout(log.try_clone().unwrap()).stderr(log).spawn().unwrap();
        Running(child)
    }

    fn until(&self, ready: impl Fn() -> bool, what: &str, log: &str) {
        assert!(wait::until(SERVE_WAIT, ready), "{what} within {} s: {}", SERVE_WAIT.as_secs(), fs::read_to_string(self.path(log)).unwrap_or_default());
    }

    fn held_queue(&self) -> bool {
        let queues = fs::read_dir(self.path("state/north/land-queue")).map(|entries| entries.flatten().map(|entry| entry.path().join("lander.lock")).collect::<Vec<_>>()).unwrap_or_default();
        queues.iter().any(|lock| fs::File::open(lock).is_ok_and(|file| rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockShared).is_err()))
    }

    fn exchange(&self, requests: &[Value]) -> Vec<Value> {
        let launch = tool_server_args(&self.candidate, &["hand-in".to_owned()]);
        let config: Value = serde_json::from_str(&launch[1]).unwrap();
        let entry = &config["mcpServers"]["north"];
        let line: Vec<&str> = std::iter::once(entry["command"].as_str().unwrap()).chain(entry["args"].as_array().unwrap().iter().filter_map(Value::as_str)).collect();
        let prefix = shell_prefix(&self.candidate).expect("a launch links worker-sandbox beside its north");
        let mut server = self.command(&prefix).arg(line.join(" ")).current_dir(&self.lane).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let mut stdin = server.stdin.take().unwrap();
        for request in requests {
            writeln!(stdin, "{request}").unwrap();
        }
        drop(stdin);
        let stdout = server.stdout.take().unwrap();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let answers: Vec<Value> = BufReader::new(stdout).lines().map_while(Result::ok).filter_map(|line| serde_json::from_str(&line).ok()).collect();
            let _ = sender.send(answers);
        });
        let answers = receiver.recv_timeout(CALL_WAIT);
        if answers.is_err() {
            let _ = server.kill();
        }
        let output = server.wait_with_output().unwrap();
        answers.unwrap_or_else(|_| panic!("the tool server answered nothing within {} s: {}", CALL_WAIT.as_secs(), String::from_utf8_lossy(&output.stderr)))
    }
}

struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn a_candidates_hand_in_tool_lands_through_its_own_build_while_the_live_link_names_an_older_one() {
    let canary = Canary::new();
    let base = git(&canary.origin, &["rev-parse", "main"]);
    fs::create_dir_all(canary.path("run/north-outside")).unwrap();
    let live_broker = UnixListener::bind(canary.path("run/north-outside/broker.sock")).unwrap();
    live_broker.set_nonblocking(true).unwrap();
    let _broker = canary.start(&["--worker-sandbox", "--broker"], &[], "broker.log");
    canary.until(|| canary.path("broker.sock").exists(), "the candidate's outside broker listened", "broker.log");
    let _lander = canary.start(&["land", "serve", "--repo", canary.main.to_str().unwrap()], &JUDGED_NORTH, "lander.log");
    canary.until(|| canary.held_queue(), "`north land serve` held the scratch queue", "lander.log");
    fs::write(canary.lane.join("worker.txt"), CHANGE).unwrap();

    let answers = canary.exchange(&[
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "claude-code", "version": "2.1.322"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "hand-in", "arguments": {"message": MESSAGE, "paths": [canary.lane.join("worker.txt")]}}}),
    ]);
    let journal = || format!("{answers:#?}\nlander: {}\nbroker: {}", fs::read_to_string(canary.path("lander.log")).unwrap_or_default(), fs::read_to_string(canary.path("broker.log")).unwrap_or_default());
    let listed: Vec<&str> = answers.iter().find(|answer| answer["id"] == 2).and_then(|answer| answer["result"]["tools"].as_array()).into_iter().flatten().filter_map(|tool| tool["name"].as_str()).collect();
    assert_eq!(listed, ["hand-in"], "{}", journal());
    let called = answers.iter().find(|answer| answer["id"] == 3).unwrap_or_else(|| panic!("no answer to the tool call: {}", journal()));
    let text = called["result"]["content"][0]["text"].as_str().unwrap_or_default();
    let landed = git(&canary.origin, &["rev-parse", "main"]);
    assert!(text.contains(&format!("Done: landed {landed} on main")) && called["result"]["isError"] == false, "the tool did not return the lander's verdict: {}", journal());
    assert_eq!(git(&canary.origin, &["rev-parse", &format!("{landed}^")]), base, "{}", journal());
    assert_eq!(git(&canary.origin, &["show", &format!("{landed}:worker.txt")]), CHANGE.trim_end(), "{}", journal());
    let reached = fs::read_to_string(canary.path("older.calls")).unwrap_or_default();
    assert!(reached.is_empty(), "the candidate ran the live runtime's north for: {reached}\n{}", journal());
    assert!(live_broker.accept().is_err(), "the candidate's sandbox asked the live runtime's broker: {}", journal());
}
