use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use serde_json::{Map, Value, json};

use super::detect::detect;
use super::events::{advisor_debug, advisor_usage, final_text, meter, parse_events, tool_calls, usage};
use super::group::{TimedOut, reap, run_check, seconds, start_group, stop_group, wait_group};
use super::json::Json;
use super::policy::{arm_minus, no_fable, policy_for_arm};
use super::pyfmt::{round_to, splitlines, strip};
use super::rows::RunRow;
use crate::error::NorthError;

const BOX_TOOLS: &str = "Bash,Read,Edit,Write,Glob,Grep";

#[derive(Clone, Debug, Default)]
pub struct Pick {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub advisor: Option<String>,
}

pub struct Item<'a> {
    pub scenario: &'a Value,
    pub arm: String,
    pub trial: usize,
    pub policy: &'a str,
    pub pick: &'a Pick,
}

fn refusal(message: String) -> NorthError {
    NorthError::Usage(format!("policy-replay: {message}"))
}

pub fn state_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state/agents/replay")
}

fn is_box(scenario: &Value) -> bool {
    scenario.get("kind").and_then(Value::as_str) == Some("box")
}

fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

fn timeout_of(value: &Value, default: f64) -> f64 {
    value.get("timeout_s").and_then(Value::as_f64).unwrap_or(default)
}

fn git(arguments: &[&str], cwd: Option<&Path>) -> Result<Vec<u8>, NorthError> {
    let mut command = Command::new("git");
    command.args(arguments);
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    let output = command.output().map_err(NorthError::Io)?;
    if !output.status.success() {
        return Err(refusal(format!("git {} failed: {}", arguments.join(" "), String::from_utf8_lossy(&output.stderr).trim())));
    }
    Ok(output.stdout)
}

fn required<'a>(scenario: &'a Value, key: &str) -> Result<&'a str, NorthError> {
    text(scenario, key).ok_or_else(|| refusal(format!("box scenario {} has no {key}", text(scenario, "id").unwrap_or("None"))))
}

type Hidden = Vec<(String, Vec<u8>)>;

fn box_checkout(scenario: &Value, work: &Path, cwd: &Path) -> Result<Hidden, NorthError> {
    let src = work.join("src");
    let src_arg = src.to_string_lossy().into_owned();
    git(&["clone", "--quiet", "--no-checkout", required(scenario, "repo")?, &src_arg], None)?;
    let commit = required(scenario, "commit")?;
    let mut hidden = Vec::new();
    for rel in scenario.get("detector").and_then(|d| d.get("tests")).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        hidden.push((rel.to_owned(), git(&["show", &format!("{commit}:{rel}")], Some(&src))?));
    }
    let archive = git(&["archive", "--format=tar", &format!("{commit}^")], Some(&src))?;
    let mut tar = Command::new("tar").arg("-x").arg("-C").arg(cwd).stdin(Stdio::piped()).spawn().map_err(NorthError::Io)?;
    tar.stdin.take().ok_or_else(|| refusal("tar has no stdin".into()))?.write_all(&archive).map_err(NorthError::Io)?;
    if !tar.wait().map_err(NorthError::Io)?.success() {
        return Err(refusal("tar -x failed".into()));
    }
    fs::remove_dir_all(&src).map_err(NorthError::Io)?;
    git(&["init", "--quiet"], Some(cwd))?;
    git(&["add", "-A"], Some(cwd))?;
    git(&["-c", "user.name=policy-replay", "-c", "user.email=policy-replay@localhost", "commit", "--quiet", "--allow-empty", "-m", "base"], Some(cwd))?;
    Ok(hidden)
}

fn write_file(root: &Path, rel: &str, content: &[u8]) -> Result<(), NorthError> {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(NorthError::Io)?;
    }
    fs::write(path, content).map_err(NorthError::Io)
}

fn prepare(scenario: &Value, work: &Path, cwd: &Path) -> Result<Hidden, NorthError> {
    if is_box(scenario) {
        return box_checkout(scenario, work, cwd);
    }
    if let Some(setup) = scenario.get("setup").and_then(Value::as_object) {
        for (rel, content) in setup {
            write_file(cwd, rel, content.as_str().unwrap_or("").as_bytes())?;
        }
    }
    Ok(Vec::new())
}

fn score(scenario: &Value, hidden: &Hidden, evs: &[Json], cwd: &Path) -> Result<bool, NorthError> {
    let detector = scenario.get("detector").ok_or_else(|| refusal(format!("scenario {} has no detector", text(scenario, "id").unwrap_or("None"))))?;
    if is_box(scenario) {
        for (rel, content) in hidden {
            write_file(cwd, rel, content)?;
        }
        return Ok(run_check(required(detector, "cmd")?, cwd, timeout_of(detector, 600.0)));
    }
    detect(detector, evs, &mut |command, timeout| run_check(command, cwd, timeout))
}

fn claude_version() -> Option<String> {
    static VERSION: OnceLock<Option<String>> = OnceLock::new();
    VERSION
        .get_or_init(|| {
            let output = Command::new("claude").arg("--version").output().ok()?;
            let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            (!version.is_empty()).then_some(version)
        })
        .clone()
}

fn run_id() -> String {
    fs::read_to_string("/proc/sys/kernel/random/uuid").map(|id| id.trim().replace('-', "")).unwrap_or_else(|_| format!("{:x}{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()), std::process::id()))
}

struct Tagged {
    stdout: String,
    stderr: String,
    code: String,
    tag: Vec<u8>,
    child: Child,
}

fn captured(file: &mut File) -> String {
    let mut bytes = Vec::new();
    let _ = file.seek(SeekFrom::Start(0));
    let _ = file.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).into_owned()
}

fn run_tagged(mut command: Command, timeout: f64) -> Result<Tagged, NorthError> {
    let id = run_id();
    command.env("POLICY_REPLAY_RUN", &id);
    let mut out = tempfile::tempfile().map_err(NorthError::Io)?;
    let mut err = tempfile::tempfile().map_err(NorthError::Io)?;
    command.stdin(Stdio::null()).stdout(out.try_clone().map_err(NorthError::Io)?).stderr(err.try_clone().map_err(NorthError::Io)?);
    let mut child = start_group(&mut command).map_err(NorthError::Io)?;
    let tag = format!("POLICY_REPLAY_RUN={id}").into_bytes();
    match wait_group(&mut child, seconds(timeout)) {
        Ok(code) => Ok(Tagged { stdout: captured(&mut out), stderr: captured(&mut err), code: code.to_string(), tag, child }),
        Err(TimedOut) => {
            stop_group(&mut child);
            Ok(Tagged { stdout: String::new(), stderr: "timeout".into(), code: "timeout".into(), tag, child })
        }
    }
}

fn pick_text(chosen: &Option<String>, scenario: &Value, key: &str, default: Option<&str>) -> Option<String> {
    chosen.clone().filter(|value| !value.is_empty()).or_else(|| text(scenario, key).filter(|value| !value.is_empty()).map(str::to_owned)).or_else(|| default.map(str::to_owned))
}

fn tail(text: &str, chars: usize) -> String {
    let skip = text.chars().count().saturating_sub(chars);
    text.chars().skip(skip).collect()
}

fn build_command(scenario: &Value, work: &Path, policy_file: &Path, model: &str, effort: &str, advisor: Option<&str>) -> Result<Command, NorthError> {
    let boxed = is_box(scenario);
    let prompt = required(scenario, if boxed { "brief" } else { "prompt" })?;
    let tools = match scenario.get("tools").and_then(Value::as_array) {
        Some(list) => list.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(","),
        None if boxed => BOX_TOOLS.to_owned(),
        None => String::new(),
    };
    let max_turns = scenario.get("max_turns").map_or_else(|| if boxed { "100".to_owned() } else { "1".to_owned() }, Value::to_string);
    let mut command = Command::new("claude");
    command.args(["-p", prompt, "--setting-sources", "project", "--append-system-prompt-file"]).arg(policy_file);
    command.args(["--model", model, "--effort", effort, "--max-turns", &max_turns]);
    command.args(["--output-format", "stream-json", "--verbose", "--no-session-persistence", "--permission-mode", "bypassPermissions"]);
    command.args(["--tools", &tools, "--strict-mcp-config", "--debug-file"]).arg(work.join("debug.log"));
    if let Some(budget) = scenario.get("max_budget_usd") {
        command.args(["--max-budget-usd", &budget.to_string()]);
    }
    if let Some(advisor) = advisor {
        command.args(["--advisor", advisor]);
    }
    command.env_clear().envs(std::env::vars().filter(|(name, _)| !name.starts_with("AGENT_") && !name.starts_with("CLAUDE_CODE_SESSION")));
    let configured = std::env::var("CLAUDE_CODE_OAUTH_TOKEN").is_ok_and(|token| !token.is_empty());
    let profile = std::env::var("CLAUDE_CONFIG_DIR").ok().and_then(|dir| Path::new(&dir).file_name().map(|name| name.to_owned()));
    if let Some(profile) = profile.filter(|_| !configured) {
        let token = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state/north/claude-tokens").join(profile);
        if token.is_file() {
            command.env("CLAUDE_CODE_OAUTH_TOKEN", fs::read_to_string(token).map_err(NorthError::Io)?.trim());
        }
    }
    if advisor.is_none() {
        command.env("CLAUDE_CODE_DISABLE_ADVISOR_TOOL", "1");
    }
    Ok(command)
}

fn first_line(text: &str) -> &str {
    splitlines(text, false).into_iter().find(|line| !strip(line).is_empty()).unwrap_or("")
}

fn merge(row: &mut Map<String, Value>, extra: &impl serde::Serialize) {
    if let Ok(Value::Object(fields)) = serde_json::to_value(extra) {
        row.extend(fields);
    }
}

pub fn run_one(scenario: &Value, arm: &str, trial: usize, policy_text: &str, pick: &Pick) -> Result<Value, NorthError> {
    let policy = if arm.starts_with("variant:") { policy_text.to_owned() } else { policy_for_arm(policy_text, &arm_minus(arm, scenario)?)? };
    let model = pick_text(&pick.model, scenario, "model", Some("opus")).unwrap_or_default();
    let effort = pick_text(&pick.effort, scenario, "effort", Some("medium")).unwrap_or_default();
    let advisor = pick_text(&pick.advisor, scenario, "advisor", None);
    no_fable(&[Some(&model), advisor.as_deref()])?;
    let started = Instant::now();
    let work = tempfile::Builder::new().prefix("policy-replay-").tempdir().map_err(NorthError::Io)?;
    let policy_file = work.path().join("policy.md");
    fs::write(&policy_file, &policy).map_err(NorthError::Io)?;
    let cwd = work.path().join("cwd");
    fs::create_dir(&cwd).map_err(NorthError::Io)?;
    let hidden = prepare(scenario, work.path(), &cwd)?;
    let mut command = build_command(scenario, work.path(), &policy_file, &model, &effort, advisor.as_deref())?;
    command.current_dir(&cwd);
    let mut run = run_tagged(command, timeout_of(scenario, 600.0))?;
    let mut row = Map::new();
    row.insert("scenario".into(), json!(text(scenario, "id")));
    row.insert("arm".into(), json!(arm));
    row.insert("trial".into(), json!(trial));
    row.insert("model".into(), json!(model));
    row.insert("effort".into(), json!(effort));
    row.insert("advisor".into(), json!(advisor));
    row.insert("claude_version".into(), json!(claude_version()));
    let evs = parse_events(&run.stdout);
    let result = usage(&evs).filter(|_| evs.iter().any(|ev| ev.is_type("result")));
    match result {
        None => {
            row.insert("pass".into(), Value::Null);
            row.insert("error".into(), json!(format!("no result (exit {}): {}", run.code, tail(&run.stderr, 300))));
        }
        Some(used) => {
            row.insert("pass".into(), json!(score(scenario, &hidden, &evs, &cwd)?));
            merge(&mut row, &used);
            merge(&mut row, &advisor_usage(&evs));
            merge(&mut row, &meter(&evs));
            let debug = advisor_debug(&fs::read_to_string(work.path().join("debug.log")).unwrap_or_default());
            let attached = debug.advisor_attached;
            row.insert("advisor_attached".into(), json!(attached));
            if let Some(resolved) = debug.advisor_model_resolved {
                row.insert("advisor_model_resolved".into(), json!(resolved));
            }
            row.insert("usd".into(), json!(used.cost_api_usd_comparison));
            row.insert("usd_basis".into(), json!("total_cost_usd list price: the ledger rate needs weekly readouts bracketing the run"));
            let subtype = used.subtype.clone();
            let failure = |message: String| {
                let mut update = Map::new();
                update.insert("pass".into(), Value::Null);
                update.insert("error".into(), json!(message));
                update
            };
            let error = if subtype.as_deref() != Some("success") || used.failure.is_some() {
                Some(match (&used.failure, subtype.as_deref()) {
                    (Some(failure), Some("success")) => failure.clone(),
                    _ => format!("run ended {} before its final answer", subtype.as_deref().unwrap_or("None")),
                })
            } else if advisor.is_some() && !attached {
                Some("advisor not attached: the debug log has no [AdvisorTool] Server-side tool enabled line".to_owned())
            } else if advisor.is_none() && attached {
                Some("control run had an advisor attached despite CLAUDE_CODE_DISABLE_ADVISOR_TOOL".to_owned())
            } else {
                None
            };
            if let Some(message) = error {
                row.extend(failure(message));
            }
            row.insert("tools_called".into(), json!(tool_calls(&evs).iter().map(|call| call.text("name")).collect::<Vec<_>>()));
            let answer = final_text(&evs);
            row.insert("first_line".into(), json!(first_line(answer).chars().take(200).collect::<String>()));
            row.insert("final_text".into(), json!(answer));
        }
    }
    row.insert("leftover_processes".into(), json!(reap(&run.tag, &mut run.child)));
    row.insert("wall_s".into(), json!(round_to(started.elapsed().as_secs_f64(), 1)));
    Ok(Value::Object(row))
}

pub fn run_row(value: &Value) -> RunRow {
    serde_json::from_value(value.clone()).unwrap_or_default()
}

pub fn run_plan(plan: &[Item], jobs: usize, out: &Path) -> Result<Vec<Value>, NorthError> {
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(NorthError::Io)?;
    }
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let (sender, receiver) = std::sync::mpsc::channel();
    let mut rows = Vec::new();
    let mut error = None;
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, 4) {
            let sender = sender.clone();
            let (next, failed) = (&next, &failed);
            scope.spawn(move || {
                while !failed.load(Ordering::SeqCst) {
                    let Some(item) = plan.get(next.fetch_add(1, Ordering::SeqCst)) else { break };
                    let result = run_one(item.scenario, &item.arm, item.trial, item.policy, item.pick);
                    if result.is_err() {
                        failed.store(true, Ordering::SeqCst);
                    }
                    if sender.send(result).is_err() {
                        break;
                    }
                }
            });
        }
        drop(sender);
        for result in receiver {
            match result {
                Ok(row) => {
                    let line = serde_json::to_string(&row).unwrap_or_default();
                    let written = fs::OpenOptions::new().create(true).append(true).open(out).and_then(|mut file| writeln!(file, "{line}"));
                    if let Err(failure) = written {
                        failed.store(true, Ordering::SeqCst);
                        error.get_or_insert(NorthError::Io(failure));
                    }
                    eprintln!("{line}");
                    rows.push(row);
                }
                Err(failure) => {
                    error.get_or_insert(failure);
                }
            }
        }
    });
    if let Some(failure) = error {
        return Err(failure);
    }
    rows.sort_by_key(|row| (text(row, "scenario").unwrap_or("").to_owned(), text(row, "arm").unwrap_or("").to_owned(), row.get("trial").and_then(Value::as_u64)));
    Ok(rows)
}
