//! The equipped T3 Code arm on a real box (north#876): T3 Code's headless server runs on the host against a worktree, and the box text is the thread's brief.
//! Every thread runs on one fixed model and effort, T3 Code's own default for Claude as a user would leave it, written as the server's default selection; north's routing never touches this arm.
//! The login is a subscription account's token and nothing else: the environment starts empty, so no API key reaches the server or the claude it spawns.
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use north_base::error::NorthError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::config;
use super::process;
use super::refusal;
use super::t3code::{self, Install};

const KEPT_ENV: [&str; 7] = ["HOME", "USER", "LOGNAME", "LANG", "TZ", "SSL_CERT_FILE", "PATH"];

/// T3 Code's own default for the Claude provider at the pinned head: `providers.claudeAgent.defaults.chat` and the `isDefault` effort of that model's profile in apps/server/src/provider/model-manifest.json.
pub const MODEL: &str = "claude-opus-5-5";
pub const EFFORT: &str = "medium";

/// One real box handed to the T3 Code arm.
pub struct Job<'a> {
    pub issue: &'a str,
    pub text: &'a str,
    pub tree: &'a Path,
    pub base: &'a Path,
    pub token: &'a Path,
    pub account: &'a str,
    pub owner: &'a Path,
    pub wall: Duration,
}

/// What the arm did, written to `run.json` under the base directory.
#[derive(Debug, Deserialize, Serialize)]
pub struct Record {
    pub issue: String,
    pub box_text: String,
    pub model: String,
    pub effort: String,
    pub account: String,
    pub config_sha256: String,
    pub thread: Option<String>,
    pub status: String,
    pub wall_s: f64,
    pub reply: Option<String>,
}

/// The thread's brief: the box text as given.
pub fn brief(text: &str) -> &str {
    text.trim()
}

fn t3(job: &Job, install: &Install, arguments: &[&str]) -> Command {
    let mut run = Command::new("sh");
    run.args(["-c", t3code::SERVER_SCRIPT, "t3", "CLAUDE_CODE_OAUTH_TOKEN"]).arg(job.token).arg(&install.node).arg(install.entry()).args(arguments);
    run.arg("--base-dir").arg(job.base).env_clear().current_dir(job.tree);
    run.envs(KEPT_ENV.iter().filter_map(|name| Some((*name, std::env::var_os(name)?))));
    run.env("DISABLE_INSTALLATION_CHECKS", "1");
    run.env("CLAUDE_CONFIG_DIR", job.base.join("config"));
    run
}

pub fn run(job: &Job, install: &Install) -> Result<Record, NorthError> {
    fs::create_dir_all(job.base).map_err(NorthError::Io)?;
    let config_sha256 = config::assemble(job.owner, &job.base.join("config"))?;
    t3code::write_settings(job.base, MODEL, EFFORT)?;
    let log = job.base.join("t3-server.log");
    let port = t3code::free_port()?.to_string();
    let tree = job.tree.display().to_string();
    let mut serve = t3(job, install, &["serve", "--port", &port, "--no-browser", &tree]);
    let out = File::options().create(true).append(true).open(&log).map_err(NorthError::Io)?;
    serve.stdin(Stdio::null()).stdout(out.try_clone().map_err(NorthError::Io)?).stderr(out);
    let mut server = process::spawn(&mut serve).map_err(NorthError::Io)?;
    let started = Instant::now();
    let outcome = thread(job, install, &mut server, &log, &tree);
    process::kill(&mut server);
    let (thread, status, reply) = outcome?;
    let record = Record { issue: job.issue.to_owned(), box_text: job.text.to_owned(), model: MODEL.to_owned(), effort: EFFORT.to_owned(), account: job.account.to_owned(), config_sha256, thread, status, wall_s: started.elapsed().as_secs_f64(), reply };
    fs::write(job.base.join("run.json"), serde_json::to_string_pretty(&record).map_err(NorthError::Json)?).map_err(NorthError::Io)?;
    Ok(record)
}

fn thread(job: &Job, install: &Install, server: &mut std::process::Child, log: &Path, tree: &str) -> Result<(Option<String>, String, Option<String>), NorthError> {
    t3code::wait_ready(log, server)?;
    let added = t3(job, install, &["project", "add", tree]).output().map_err(NorthError::Io)?;
    if !added.status.success() {
        return Err(refusal(format!("t3 project add: {}", String::from_utf8_lossy(&added.stderr).trim())));
    }
    let reply = job.base.join("thread.json");
    let mut start = t3(job, install, &["thread", "start", tree, brief(job.text), "--wait", "--json"]);
    start.stdout(File::create(&reply).map_err(NorthError::Io)?).stderr(Stdio::null());
    let mut child = process::spawn(&mut start).map_err(NorthError::Io)?;
    let ended = process::wait_until(&mut child, Instant::now() + job.wall);
    let printed: Value = fs::read_to_string(&reply).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null);
    let status = match (ended, printed["status"].as_str()) {
        (None, _) => "wall cap".to_owned(),
        (Some(_), Some(status)) => status.to_owned(),
        (Some(code), None) => format!("exit {code}"),
    };
    Ok((printed["threadId"].as_str().map(str::to_owned), status, printed["reply"].as_str().map(str::to_owned)))
}

pub fn token_path(state: &Path, account: &str) -> PathBuf {
    let file = state.join("claude-tokens").join(account);
    fs::canonicalize(&file).unwrap_or(file)
}
