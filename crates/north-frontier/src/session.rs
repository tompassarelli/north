use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

use north_base::error::NorthError;
use north_claude_stream::encode_user_message;
use north_fleet::fleet::model::{Account, ApiKey, Effort, LaunchRequest, ProviderName, Role};
use north_fleet::fleet::prompt::{PromptInputs, PromptMode, StandingOrders, render};
use north_fleet::fleet::runtime::{DEFAULT_FOREGROUND_CAP_SECS, Opening, Runtime, new_uuid, now_ms};
use north_providers::providers::claude::api_pool;
use north_providers::providers::claude::fleet::Claude;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::billing::Spent;
use super::process;
use super::sandbox::Sandbox;
use super::task::Task;
use super::{home, refusal};
use crate::rows::Arm;

pub const CONTINUATION: &str = "No one will answer questions during this run. Make any decision yourself and finish the task.";
const MAX_INTERVENTIONS: u32 = 2;
const QUEUE_WAIT: Duration = Duration::from_secs(3 * 3600);
const KEPT_ENV: [&str; 6] = ["HOME", "USER", "LOGNAME", "LANG", "TZ", "SSL_CERT_FILE"];

pub struct Limits {
    pub budget_usd: f64,
    pub wall: Duration,
    pub idle: Duration,
}

/// What one arm did on one task, as observed from outside the sandbox.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunRecord {
    pub task: String,
    pub arm: Arm,
    pub sample: u32,
    pub model: String,
    pub effort: String,
    pub session: String,
    pub wall_s: f64,
    pub cost_usd: f64,
    pub turns: i64,
    pub interventions: u32,
    pub ended: String,
    pub final_text: String,
    pub harness: String,
}

fn worker_prompt(name: &str) -> Result<String, NorthError> {
    let orders = StandingOrders::in_state(&home().join(".local/state/north")).read()?;
    Ok(render(&PromptInputs { session: name, role: Role::Worker, mode: PromptMode::North, orders: &orders, foreground_cap_secs: DEFAULT_FOREGROUND_CAP_SECS, extra: None }))
}

fn request(task: &Task, name: &str, tree: &Path, brief: &str) -> Result<LaunchRequest, NorthError> {
    Ok(LaunchRequest {
        verification: false,
        name: name.to_owned(),
        provider: ProviderName::new("claude"),
        account: "api".into(),
        cwd: tree.to_owned(),
        model: Some(task.model.clone()),
        effort: Some(Effort::parse(&task.effort)?),
        prompt: brief.to_owned(),
        system_prompt: None,
        foreground_cap_secs: None,
        prompt_mode: PromptMode::North,
        idle_exit_minutes: None,
        parent: None,
        task_ref: Some(task.issue.clone()),
        claim_box: None,
        placement: None,
        machine: None,
        capacity_need: None,
        role: Some(Role::Worker),
        resume: None,
        compact_window: None,
        slots: Default::default(),
    })
}

/// The dev shell's environment for the clone, the toolchain both arms build with.
fn dev_env(tree: &Path) -> Result<Vec<(String, OsString)>, NorthError> {
    let output = Command::new("nix").args(["develop", "--command", "env", "-0"]).current_dir(tree).env("PATH", super::sandbox::host_path()).output().map_err(NorthError::Io)?;
    if !output.status.success() {
        return Err(refusal(format!("nix develop in {}: {}", tree.display(), String::from_utf8_lossy(&output.stderr).trim())));
    }
    let dropped = |name: &str| name.starts_with("NORTH_") || name.starts_with("CLAUDE") || name.starts_with("ANTHROPIC") || name.starts_with("GH_") || name.starts_with("GITHUB") || name.starts_with("GIT_") || name.starts_with("SSH_") || name == "THREADS_ACTOR" || name.starts_with("AGENT_") || name == "RUSTC_WRAPPER";
    Ok(output.stdout.split(|byte| *byte == 0).filter_map(|entry| std::str::from_utf8(entry).ok()?.split_once('=')).filter(|(name, _)| !dropped(name)).map(|(name, value)| (name.to_owned(), OsString::from(value))).collect())
}

/// A command inside the arm's sandbox: its view of `~/code`, the key, `bound` read-only, and the dev shell's toolchain behind the shims.
pub(crate) fn sandboxed(sandbox: &Sandbox, key: &Path, bound: &[PathBuf], tail: Vec<String>) -> Result<Command, NorthError> {
    let mut argv = sandbox.bwrap(key);
    for path in bound {
        argv.extend(["--ro-bind".into(), path.display().to_string(), path.display().to_string()]);
    }
    argv.extend(["nice".into(), "-n".into(), "10".into()]);
    argv.extend(tail);
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]).env_clear();
    command.envs(KEPT_ENV.iter().filter_map(|name| Some((*name, std::env::var_os(name)?))));
    let mut env = dev_env(&sandbox.tree)?;
    let path = env.iter().find(|(name, _)| name == "PATH").map(|(_, value)| value.clone()).unwrap_or_default();
    let mut joined = OsString::from(sandbox.shims.as_os_str());
    joined.push(":");
    joined.push(path);
    env.retain(|(name, _)| name != "PATH" && name != "HOME" && name != "TMPDIR");
    command.envs(env).env("PATH", joined).env("DISABLE_AUTOUPDATER", "1");
    Ok(command)
}

fn command(arm: &Attempt, sandbox: &Sandbox, name: &str, conversation: &str) -> Result<Command, NorthError> {
    let Attempt { task, harness, key, plan, brief, limits, .. } = *arm;
    let (token_file, api) = if plan { (Some(key.to_owned()), None) } else { (None, Some(ApiKey { org: "frontier".into(), file: key.to_owned() })) };
    let account = Account { provider: ProviderName::new("claude"), label: "api".into(), home: sandbox.config.clone(), token_file, api };
    let instructions = if harness == Arm::North { worker_prompt(name)? } else { String::new() };
    let spec = Claude.host(&account, &request(task, name, &sandbox.tree, brief)?, Opening::Fresh(Some(conversation)), &instructions)?;
    let mut tail = spec.argv;
    tail.extend(["--disallowedTools".into(), "WebFetch,WebSearch".into(), "--max-budget-usd".into(), format!("{:.2}", limits.budget_usd)]);
    let mut command = sandboxed(sandbox, key, &[], tail)?;
    let fleet_env = spec.env.into_iter().filter(|(name, _)| !name.starts_with("GIT_"));
    match harness {
        Arm::North => command.envs(fleet_env),
        Arm::Plain | Arm::T3Code => command.envs(fleet_env.filter(|(name, _)| name == "CLAUDE_CONFIG_DIR")),
    };
    Ok(command)
}

/// The arm changed something: a commit or an uncommitted edit in any checkout, or a push to its origin.
pub fn changed(sandbox: &Sandbox, base: &str) -> bool {
    sandbox.pushed(base).is_some() || sandbox.worktrees().iter().any(|tree| sandbox.touched(tree, base))
}

pub const CREDIT: &str = "credit refused";

pub(crate) fn refused_for_credit(result: &Value) -> bool {
    result["is_error"] == true && result["result"].as_str().is_some_and(api_pool::low_credit)
}

pub(crate) fn asks(text: &str) -> bool {
    let last = text.lines().rev().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    let first = text.trim_start();
    let another_machine = text.lines().map(str::trim).filter_map(|line| line.strip_prefix("Stop:")).any(|stop| ["client", "host"].iter().any(|named| stop.trim_start().starts_with(named)));
    last.ends_with('?') || ((first.starts_with("Not done:") || first.starts_with("Blocked:")) && !another_machine)
}

pub(crate) struct Stream {
    pub(crate) reader: BufReader<File>,
    pub(crate) results: Vec<Value>,
    pub(crate) first_event: Option<Instant>,
    pub(crate) last_event: Instant,
    pub(crate) waited: Duration,
}

impl Stream {
    pub(crate) fn drain(&mut self) {
        let mut line = String::new();
        while self.reader.read_line(&mut line).is_ok_and(|read| read > 0) {
            if line.ends_with('\n') {
                if let Ok(event) = serde_json::from_str::<Value>(line.trim()) {
                    self.last_event = Instant::now();
                    self.first_event.get_or_insert(self.last_event);
                    if event["type"] == "result" {
                        self.results.push(event);
                    }
                }
                line.clear();
            }
        }
    }
}

fn send(stdin: &mut ChildStdin, text: &str) -> Result<(), NorthError> {
    writeln!(stdin, "{}", encode_user_message(&new_uuid()?, text)).map_err(NorthError::Io)?;
    stdin.flush().map_err(NorthError::Io)
}

#[derive(Clone, Copy)]
pub struct Attempt<'a> {
    pub task: &'a Task,
    pub harness: Arm,
    pub sample: u32,
    pub key: &'a Path,
    pub plan: bool,
    pub brief: &'a str,
    pub limits: &'a Limits,
}

pub fn run(arm: &Attempt, sandbox: &Sandbox) -> Result<(RunRecord, Spent), NorthError> {
    let name = format!("frontier-{}-{}-{}", arm.task.id, arm.harness.label(), arm.sample);
    let conversation = new_uuid()?;
    let stream_path = sandbox.root.join("stream.jsonl");
    let out = OpenOptions::new().create(true).append(true).open(&stream_path).map_err(NorthError::Io)?;
    let errors = File::create(sandbox.root.join("stderr.log")).map_err(NorthError::Io)?;
    let mut command = command(arm, sandbox, &name, &conversation)?;
    command.stdin(Stdio::piped()).stdout(out).stderr(errors);
    let started = Instant::now();
    let started_ms = now_ms();
    let mut child = process::spawn(&mut command).map_err(NorthError::Io)?;
    let mut stdin = child.stdin.take().ok_or_else(|| refusal("claude took no stdin"))?;
    send(&mut stdin, arm.brief)?;
    let mut stream = Stream { reader: BufReader::new(File::open(&stream_path).map_err(NorthError::Io)?), results: Vec::new(), first_event: None, last_event: started, waited: Duration::ZERO };
    let (ended, interventions) = supervise(arm, sandbox, &mut child, stdin, &mut stream, started)?;
    stream.drain();
    if stream.results.is_empty() && ended == "exited" {
        let errors = fs::read_to_string(sandbox.root.join("stderr.log")).unwrap_or_default();
        let tail: String = errors.lines().rev().take(3).collect::<Vec<_>>().join(" / ");
        return Err(refusal(format!("the session ended with no result: {tail}")));
    }
    let ended_ms = now_ms();
    let spent = Spent::of(&name, &stream.results, started_ms, ended_ms);
    Ok((record(arm, conversation, &stream, ended, interventions)?, spent))
}

fn supervise(arm: &Attempt, sandbox: &Sandbox, child: &mut std::process::Child, mut stdin: ChildStdin, stream: &mut Stream, started: Instant) -> Result<(String, u32), NorthError> {
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
            process::kill(child);
            break "wall cap".to_owned();
        }
        if stream.results.iter().any(refused_for_credit) {
            process::kill(child);
            break CREDIT.to_owned();
        }
        let idle = stream.results.len() > answered && stream.last_event.elapsed() >= arm.limits.idle;
        if !idle {
            continue;
        }
        answered = stream.results.len();
        let last = stream.results.last().map(|result| result["result"].as_str().unwrap_or("").to_owned()).unwrap_or_default();
        let capped = stream.results.last().is_some_and(|result| result["subtype"].as_str().is_some_and(|kind| kind.contains("budget")));
        if !capped && interventions < MAX_INTERVENTIONS && (asks(&last) || !changed(sandbox, &arm.task.base)) {
            interventions += 1;
            stream.waited += stream.last_event.elapsed();
            send(&mut stdin, CONTINUATION)?;
            continue;
        }
        drop(stdin);
        let code = process::wait_until(child, Instant::now() + Duration::from_secs(60));
        break if capped { "budget cap".to_owned() } else { format!("stopped (exit {})", code.map_or("killed".into(), |code| code.to_string())) };
    };
    Ok((ended, interventions))
}

pub(crate) fn record(arm: &Attempt, conversation: String, stream: &Stream, ended: String, interventions: u32) -> Result<RunRecord, NorthError> {
    let wall_s = stream.first_event.map_or(0.0, |first| stream.last_event.duration_since(first).saturating_sub(stream.waited).as_secs_f64());
    let last = stream.results.last().cloned().unwrap_or(Value::Null);
    let harness = match arm.harness {
        Arm::North => fs::read_link(home().join(".claude/CLAUDE.md")).map_or_else(|_| "unknown".into(), |path| path.display().to_string()),
        Arm::Plain => "none".into(),
        Arm::T3Code => format!("t3code 32a0531: T3 Code's own server and Claude adapter, run headless, all of its tools but the {} it cannot run in the sandbox ({}); effort is T3 Code's default", crate::t3code::DROPPED.len(), crate::t3code::DROPPED.join(" ")),
    };
    Ok(RunRecord {
        task: arm.task.id.clone(),
        arm: arm.harness,
        sample: arm.sample,
        model: arm.task.model.clone(),
        effort: arm.task.effort.clone(),
        session: conversation,
        wall_s,
        cost_usd: stream.results.iter().filter_map(|result| result["total_cost_usd"].as_f64()).fold(0.0, f64::max),
        turns: stream.results.iter().filter_map(|result| result["num_turns"].as_i64()).sum(),
        interventions,
        ended,
        final_text: last["result"].as_str().unwrap_or("").to_owned(),
        harness,
    })
}

pub fn record_path(root: &Path) -> PathBuf {
    root.join("run.json")
}
