//! The T3 Code arm: T3 Code's own server (T3 Tools Inc., MIT, github.com/pingdotgg/t3code at 32a0531748c44b25072d7495ce0b9bd7e9450a87)
//! runs headless inside the arm's sandbox, and the task is a thread it starts through its own CLI, so Claude Code gets the instructions,
//! MCP tools and adapter options T3 Code sends. The only seam is a `claude` shim that runs the same Claude Code binary as the other
//! arms, copies its stream to `stream.jsonl` for cost, wall time and the leak screen, and applies the cap on cost and the web-tool ban.
//! T3 Code's effort for the model is its own default: its CLI cannot set one.
use std::fs::{self, File, OpenOptions};
use std::io::BufReader;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use north_base::error::NorthError;
use north_fleet::fleet::runtime::now_ms;
use serde_json::Value;

use super::billing::Spent;
use super::process;
use super::refusal;
use super::sandbox::Sandbox;
use super::session::{Attempt, CONTINUATION, CREDIT, RunRecord, Stream, asks, changed, record, refused_for_credit, sandboxed};

const PROVIDER: &str = "claudeAgent";
const READY: &str = "T3 Code server is ready";
const START_WAIT: Duration = Duration::from_secs(180);
const QUEUE_WAIT: Duration = Duration::from_secs(3 * 3600);
const MAX_INTERVENTIONS: u32 = 2;
const SERVER_SCRIPT: &str = "export \"$1=$(cat \"$2\")\"; shift 2; exec \"$@\"";

/// A built T3 Code checkout at the pinned head and the node that runs it, both read-only inside the sandbox.
pub struct Install {
    dir: PathBuf,
    node: PathBuf,
}

impl Install {
    pub fn new(dir: &str, node: Option<&str>) -> Self {
        Self { dir: PathBuf::from(dir), node: PathBuf::from(node.unwrap_or("node")) }
    }

    fn entry(&self) -> PathBuf {
        self.dir.join("apps/server/dist/bin.mjs")
    }

    fn node_root(&self) -> Option<PathBuf> {
        self.node.ancestors().find(|dir| dir.parent().is_some_and(|up| up == Path::new("/nix/store"))).map(Path::to_path_buf)
    }
}

/// T3 Code's own tools that cannot run in the arm's sandbox, each dropped from the model's view and never stubbed: the server drives a Playwright browser, an agent-device shim and a headless Chrome, none of which the sandbox holds.
pub(crate) const DROPPED: [&str; 20] = [
    "preview_dialog", "preview_status", "preview_open", "preview_navigate", "preview_resize", "preview_set_appearance", "preview_act", "preview_evaluate", "preview_wait_for", "preview_recording_start",
    "preview_recording_stop", "preview_snapshot", "t3_preview_list", "t3_preview_close", "device_list", "device_open", "device_close", "device_screenshot", "html_render", "html_preview",
];

fn claude_shim(sandbox: &Sandbox, arm: &Attempt) -> Result<(), NorthError> {
    let real = north_fleet::fleet::install::runtime("claude");
    let stream = sandbox.root.join("stream.jsonl");
    let banned: Vec<String> = ["WebFetch".to_owned(), "WebSearch".to_owned()].into_iter().chain(DROPPED.iter().map(|tool| format!("mcp__t3-code__{tool}"))).collect();
    let login = if arm.plan { format!("CLAUDE_CODE_OAUTH_TOKEN=$(cat {}); export CLAUDE_CODE_OAUTH_TOKEN\n", arm.key.display()) } else { String::new() };
    let script = format!(
        "#!/bin/sh\n{login}case \"$*\" in\n  *stream-json*) {real} --max-budget-usd {:.2} --disallowedTools {} \"$@\" | tee -a {stream} ;;\n  *) exec {real} \"$@\" ;;\nesac\n",
        arm.limits.budget_usd,
        banned.join(","),
        real = real.display(),
        stream = stream.display()
    );
    north_base::foundation::shim::write_executable(&sandbox.shims.join("claude"), script);
    File::create(&stream).map_err(NorthError::Io)?;
    Ok(())
}

fn free_port() -> Result<u16, NorthError> {
    Ok(TcpListener::bind("127.0.0.1:0").and_then(|listener| listener.local_addr()).map_err(NorthError::Io)?.port())
}

fn bound(install: &Install) -> Vec<PathBuf> {
    [Some(install.dir.clone()), install.node_root()].into_iter().flatten().collect()
}

fn server(arm: &Attempt, sandbox: &Sandbox, install: &Install, home: &Path, port: u16, log: &Path) -> Result<Command, NorthError> {
    let tail: Vec<String> = ["sh", "-c", SERVER_SCRIPT, "t3", if arm.plan { "CLAUDE_CODE_OAUTH_TOKEN" } else { "ANTHROPIC_API_KEY" }, &arm.key.display().to_string(), &install.node.display().to_string(), &install.entry().display().to_string(), "serve", "--base-dir", &home.display().to_string(), "--port", &port.to_string(), "--no-browser", &sandbox.inside().display().to_string()].map(str::to_owned).to_vec();
    let mut command = sandboxed(sandbox, arm.key, &bound(install), tail)?;
    command.env("CLAUDE_CONFIG_DIR", &sandbox.config).env("DISABLE_INSTALLATION_CHECKS", "1");
    let out = OpenOptions::new().create(true).append(true).open(log).map_err(NorthError::Io)?;
    command.stdin(Stdio::null()).stdout(out.try_clone().map_err(NorthError::Io)?).stderr(out);
    Ok(command)
}

fn cli(arm: &Attempt, sandbox: &Sandbox, install: &Install, home: &Path, arguments: &[&str]) -> Result<String, NorthError> {
    let mut tail = vec![install.node.display().to_string(), install.entry().display().to_string()];
    tail.extend(arguments.iter().map(|word| (*word).to_owned()));
    tail.extend(["--base-dir".into(), home.display().to_string()]);
    let mut command = sandboxed(sandbox, arm.key, &bound(install), tail)?;
    command.env("CLAUDE_CONFIG_DIR", &sandbox.config);
    let output = command.output().map_err(NorthError::Io)?;
    if !output.status.success() {
        return Err(refusal(format!("t3 {}: {}", arguments.first().copied().unwrap_or(""), String::from_utf8_lossy(&output.stderr).trim())));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn wait_ready(log: &Path, child: &mut std::process::Child) -> Result<(), NorthError> {
    let deadline = Instant::now() + START_WAIT;
    while Instant::now() < deadline {
        if fs::read_to_string(log).is_ok_and(|text| text.contains(READY)) {
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(refusal(format!("the T3 Code server exited at start ({status}): {}", tail_of(log))));
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    Err(refusal("the T3 Code server was not ready in three minutes"))
}

fn tail_of(log: &Path) -> String {
    fs::read_to_string(log).unwrap_or_default().lines().rev().take(3).collect::<Vec<_>>().join(" / ")
}

pub fn run(arm: &Attempt, sandbox: &Sandbox, install: &Install) -> Result<(RunRecord, Spent), NorthError> {
    let name = format!("frontier-{}-{}-{}", arm.task.id, arm.harness.label(), arm.sample);
    claude_shim(sandbox, arm)?;
    let home = sandbox.root.join("t3home");
    let log = sandbox.root.join("t3-server.log");
    let started = Instant::now();
    let started_ms = now_ms();
    let mut child = process::spawn(&mut server(arm, sandbox, install, &home, free_port()?, &log)?).map_err(NorthError::Io)?;
    let outcome = drive(arm, sandbox, install, &home, &mut child, started);
    process::kill(&mut child);
    let (stream, ended, interventions, thread) = outcome?;
    if stream.results.is_empty() {
        return Err(refusal(format!("the T3 Code thread ended with no result ({ended}): {}", tail_of(&log))));
    }
    let spent = Spent::of(&name, &stream.results, started_ms, now_ms());
    Ok((record(arm, thread, &stream, ended, interventions)?, spent))
}

fn drive(arm: &Attempt, sandbox: &Sandbox, install: &Install, home: &Path, child: &mut std::process::Child, started: Instant) -> Result<(Stream, String, u32, String), NorthError> {
    let here = sandbox.inside().display().to_string();
    wait_ready(&sandbox.root.join("t3-server.log"), child)?;
    cli(arm, sandbox, install, home, &["project", "add", &here])?;
    let launched = cli(arm, sandbox, install, home, &["thread", "start", &here, arm.brief, "--provider", PROVIDER, "--model", &arm.task.model, "--json"])?;
    let thread = serde_json::from_str::<Value>(&launched).ok().and_then(|reply| reply["threadId"].as_str().map(str::to_owned)).ok_or_else(|| refusal(format!("t3 thread start printed no thread id: {launched}")))?;
    let mut stream = Stream { reader: BufReader::new(File::open(sandbox.root.join("stream.jsonl")).map_err(NorthError::Io)?), results: Vec::new(), first_event: None, last_event: started };
    let mut interventions = 0;
    let mut answered = 0;
    let ended = loop {
        std::thread::sleep(Duration::from_secs(2));
        stream.drain();
        if let Ok(Some(_)) = child.try_wait() {
            break "exited".to_owned();
        }
        let deadline = stream.first_event.map_or(started + QUEUE_WAIT, |first| first + arm.limits.wall);
        if Instant::now() >= deadline {
            break "wall cap".to_owned();
        }
        if stream.results.iter().any(refused_for_credit) {
            break CREDIT.to_owned();
        }
        if stream.results.len() <= answered || stream.last_event.elapsed() < arm.limits.idle {
            continue;
        }
        answered = stream.results.len();
        let last = stream.results.last().map(|result| result["result"].as_str().unwrap_or("").to_owned()).unwrap_or_default();
        let capped = stream.results.last().is_some_and(|result| result["subtype"].as_str().is_some_and(|kind| kind.contains("budget")));
        if !capped && interventions < MAX_INTERVENTIONS && (asks(&last) || !changed(sandbox, &arm.task.base)) {
            interventions += 1;
            cli(arm, sandbox, install, home, &["thread", "send", &thread, CONTINUATION])?;
            continue;
        }
        break if capped { "budget cap".to_owned() } else { "stopped (exit 0)".to_owned() };
    };
    stream.drain();
    Ok((stream, ended, interventions, thread))
}
