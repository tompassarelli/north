use std::fs::{self, File};
use std::io::Write as _;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::cli::Settings;
use crate::error::{Result, RunError};
use crate::gate::{self, Headroom};
use crate::images::{self, Images};
use crate::manifest::{HARBOR_COMMIT, TASKS_COMMIT, Task};
use crate::plan::{EFFORT, Harness, MODEL, Trial};
use crate::proc::{self, EVAL_X};
use crate::transcript::{self, Reading};
use crate::wait;

const EVAL_X_BIN: &str = "/eval-x-bin";
const TRIAL_MOUNT: &str = "/eval-x";
const ARM_GRACE: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attempt {
    pub rate_limited: bool,
    pub effort: Option<String>,
}

pub struct Live<'a> {
    pub settings: &'a Settings,
    pub account: &'a str,
    pub token_file: &'a Path,
    pub claude: &'a Path,
    pub run_id: &'a str,
    pub host: String,
    pub stamps: Value,
    pub images: Images,
    pub rows: Mutex<File>,
}

impl Live<'_> {
    fn work(&self, name: &str) -> PathBuf {
        self.settings.work.join(name)
    }

    pub fn record(&self, row: &Value) -> Result<()> {
        let mut rows = self.rows.lock().unwrap_or_else(PoisonError::into_inner);
        writeln!(rows, "{row}")?;
        println!("{row}");
        Ok(())
    }
}

impl Live<'_> {
    pub fn attempt(&self, trial: &Trial, pass: u32, attempt: u32) -> Result<Attempt> {
        let image = trial.task.image_ref();
        let gates = self.gates()?;
        let digest_before = self.images.check(&image)?;
        let job = format!("{}-{}-p{pass}a{attempt}", self.run_id, trial.id);
        let started = gate::now();
        let session = self.session(trial, &job)?;
        let finished = gate::now();
        let digest = self.images.check(&image)?.or(digest_before);
        let rate_limited = session.reading.rate_limited;
        let effort = session.reading.effort();
        let policy = fs::read_to_string(&self.settings.policy).unwrap_or_default();
        let (first_request, plain) = session.first_request(&policy);
        let invalid_reason = session.invalid_reason(trial.harness == Harness::T3 && !plain);
        self.record(&json!({
            "kind": "trial",
            "run": self.run_id,
            "trial": trial.id,
            "job": job,
            "phase": trial.phase.label(),
            "pass": pass,
            "attempt": attempt,
            "task": trial.task.name,
            "arm": trial.harness.arm_name(),
            "label": "HOST-TOOLS",
            "large": trial.task.is_large(),
            "virtual_machine": trial.task.is_virtual_machine(),
            "seed": format!("{:016x}", trial.seed),
            "account": self.account,
            "host": self.host,
            "load_at_start": gates.load.loadavg,
            "load_wait_s": gates.load.waited_s,
            "headroom": gates.headroom_row(),
            "tasks_commit": TASKS_COMMIT,
            "harbor_commit": HARBOR_COMMIT,
            "stamps": self.stamps,
            "image": image,
            "image_digest": digest,
            "image_bytes": images::size_bytes(&image),
            "harbor_status": session.harbor_status,
            "harbor": session.harbor,
            "started": started,
            "finished": finished,
            "wall_s": finished.saturating_sub(started),
            "arm_result": session.arm,
            "session": session.reading.row(),
            "first_request": first_request,
            "disk_kb": session.disk_kb,
            "valid": invalid_reason.is_none(),
            "invalid_reason": invalid_reason,
        }))?;
        Ok(Attempt {
            rate_limited,
            effort,
        })
    }
}

struct Gates {
    load: wait::LoadReading,
    headroom: Headroom,
    headroom_waited_s: u64,
}

impl Gates {
    fn headroom_row(&self) -> Value {
        json!({
            "five_hour_used": self.headroom.five_hour_used,
            "week_used": self.headroom.week_used,
            "reading_at": self.headroom.reading_at,
            "wait_s": self.headroom_waited_s,
        })
    }
}

struct Session {
    harbor_status: Value,
    arm: Option<Value>,
    harbor: Option<Value>,
    reading: Reading,
    disk_kb: Value,
    workspace_top: String,
}

impl Session {
    fn first_request(&self, policy: &str) -> (Value, bool) {
        let (searched, found) = self.reading.policy_lines_found(policy);
        let files = self.reading.instruction_files();
        let recorded = self.reading.first_request_bytes() > 0;
        let inside = files
            .iter()
            .all(|file| file.starts_with(&self.workspace_top));
        let plain = recorded && found == 0 && inside;
        let row = json!({
            "bytes": self.reading.first_request_bytes(),
            "instruction_files": files,
            "policy_lines_searched": searched,
            "policy_lines_found": found,
        });
        (row, plain)
    }

    fn invalid_reason(&self, plain_arm_with_policy: bool) -> Option<&'static str> {
        if self.reading.rate_limited {
            Some("rate_limited")
        } else if self.harbor.is_none() {
            Some("no_harbor_result")
        } else if self.arm.is_none() {
            Some("no_arm_result")
        } else if plain_arm_with_policy {
            Some("policy_in_plain_arm")
        } else {
            None
        }
    }
}

impl Live<'_> {
    fn gates(&self) -> Result<Gates> {
        let load = wait::for_load(self.settings.max_load)?;
        let (headroom, headroom_waited_s) = wait::for_headroom(self.settings, self.account)?;
        Ok(Gates {
            load,
            headroom,
            headroom_waited_s,
        })
    }

    fn session(&self, trial: &Trial, job: &str) -> Result<Session> {
        let trial_dir = self.work("trials").join(job);
        fs::create_dir_all(&trial_dir)?;
        let trial_dir = fs::canonicalize(&trial_dir)?;
        let arm_dir = self.work("arms").join(job);
        fs::create_dir_all(&arm_dir)?;
        let workspace = seed_workspace(&trial.task.name)?;
        let mounts = mounts_for(&workspace, &trial_dir, &self.work("bin"))?;
        let sentence = instruction_sentence(&workspace);
        let mut arm = self.spawn_arm(trial, &trial_dir, &arm_dir, &workspace.cwd)?;
        let mut broker = match start_broker(&trial_dir, &arm_dir) {
            Ok(broker) => broker,
            Err(error) => {
                stop_group(&mut arm)?;
                return Err(error);
            }
        };
        let harbor_status = run_harbor(self, &trial.task, job, &mounts, &sentence, &arm_dir);
        let broker_stopped = stop_broker(&trial_dir, &mut broker);
        settle_arm(&mut arm, &trial_dir)?;
        broker_stopped?;
        let job_dir = self.work("jobs").join(job);
        Ok(Session {
            harbor_status: harbor_status?,
            arm: read_json(&arm_dir.join("arm.json")),
            harbor: harbor_result(&job_dir),
            reading: transcript::read(&arm_dir.join("transcript.jsonl")),
            disk_kb: json!({
                "workspace": disk_kb(Path::new(&workspace.host_top)),
                "trial": disk_kb(&trial_dir),
                "arm": disk_kb(&arm_dir),
                "job": disk_kb(&job_dir),
            }),
            workspace_top: workspace.host_top,
        })
    }

    fn spawn_arm(
        &self,
        trial: &Trial,
        trial_dir: &Path,
        arm_dir: &Path,
        cwd: &str,
    ) -> Result<Child> {
        let log = proc::create(&arm_dir.join("driver.log"))?;
        let log_err = log.try_clone()?;
        let child = proc::spawn(
            Command::new(format!("{EVAL_X}/arm"))
                .args(["--arm", trial.harness.arm_name(), "--trial"])
                .arg(trial_dir)
                .arg("--state")
                .arg(arm_dir)
                .args([
                    "--cwd",
                    cwd,
                    "--account",
                    self.account,
                    "--model",
                    MODEL,
                    "--effort",
                    EFFORT,
                ])
                .arg("--limit")
                .arg(trial.task.agent_timeout_s.to_string())
                .env("EVAL_T_TOKEN_FILE", self.token_file)
                .env("EVAL_T_CLAUDE", self.claude)
                .process_group(0)
                .stdin(Stdio::null())
                .stdout(log)
                .stderr(log_err),
        )?;
        Ok(child)
    }
}

struct Workspace {
    cwd: String,
    top: String,
    host_top: String,
    mounts: Value,
}

fn seed_workspace(task: &str) -> Result<Workspace> {
    let output = proc::output(Command::new(format!("{EVAL_X}/workspace/seed")).arg(task))?;
    if !output.status.success() {
        return Err(RunError::Failed(format!(
            "workspace seed for {task} exited {}",
            output.status
        )));
    }
    let value: Value = serde_json::from_slice(&output.stdout)?;
    let text = |key: &str| -> Result<String> {
        value
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| RunError::Failed(format!("workspace seed for {task} has no {key}")))
    };
    let top = text("top")?;
    Ok(Workspace {
        cwd: text("cwd")?,
        host_top: format!("{}{top}", text("root")?),
        top,
        mounts: value.get("mounts").cloned().unwrap_or_else(|| json!([])),
    })
}

fn mounts_for(workspace: &Workspace, trial_dir: &Path, bin: &Path) -> Result<String> {
    let mut mounts = workspace
        .mounts
        .as_array()
        .cloned()
        .ok_or_else(|| RunError::Failed("workspace seed mounts are not an array".to_owned()))?;
    mounts.push(json!({"type": "bind", "source": trial_dir, "target": TRIAL_MOUNT}));
    mounts.push(json!({"type": "bind", "source": bin, "target": EVAL_X_BIN, "read_only": true}));
    Ok(Value::Array(mounts).to_string())
}

fn instruction_sentence(workspace: &Workspace) -> String {
    format!(
        "The task's directory {} is your session's directory {}; shell commands run inside the task's machine, where both paths name the same files.",
        workspace.top, workspace.host_top
    )
}

fn run_harbor(
    live: &Live<'_>,
    task: &Task,
    job: &str,
    mounts: &str,
    sentence: &str,
    arm_dir: &Path,
) -> Result<Value> {
    let handoff = proc::output(&mut Command::new(format!("{EVAL_X}/handoff/args")))?;
    if !handoff.status.success() {
        return Err(RunError::Failed(format!(
            "handoff args exited {}",
            handoff.status
        )));
    }
    let extra: Vec<String> = String::from_utf8_lossy(&handoff.stdout)
        .lines()
        .map(str::to_owned)
        .collect();
    let log = proc::create(&arm_dir.join("harbor.log"))?;
    let log_err = log.try_clone()?;
    let status = proc::status(
        Command::new(format!("{EVAL_X}/host"))
            .args(["harbor", "run", "--path"])
            .arg(live.work("tasks").join(&task.name))
            .args([
                "--env",
                "podman",
                "--n-concurrent",
                "1",
                "--yes",
                "--jobs-dir",
            ])
            .arg(live.work("jobs"))
            .args([
                "--job-name",
                job,
                "--mounts-json",
                mounts,
                "--extra-instruction",
                sentence,
            ])
            .args(extra)
            .stdout(log)
            .stderr(log_err),
    )?;
    Ok(json!(status.code()))
}

fn broker_command() -> PathBuf {
    Path::new(EVAL_X).join("relay/broker")
}

fn start_broker(trial_dir: &Path, arm_dir: &Path) -> Result<Child> {
    let log = proc::create(&arm_dir.join("broker.log"))?;
    let log_err = log.try_clone()?;
    let child = proc::spawn(
        Command::new(broker_command())
            .arg(trial_dir)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(log_err),
    )?;
    Ok(child)
}

fn stop_broker(trial_dir: &Path, broker: &mut Child) -> Result<()> {
    let stopped = proc::status(
        Command::new(broker_command())
            .arg("stop")
            .arg(trial_dir)
            .stdin(Stdio::null()),
    )?;
    if !stopped.success() {
        eprintln!("eval-x: relay broker stop exited {stopped}; ending its process");
    }
    stop_child(broker)
}

fn stop_child(child: &mut Child) -> Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    proc::status(Command::new("kill").args(["-TERM", &child.id().to_string()]))?;
    child.wait()?;
    Ok(())
}

fn settle_arm(arm: &mut Child, trial_dir: &Path) -> Result<()> {
    let handed_off = trial_dir.join("prompt.json").exists();
    let deadline = Instant::now()
        + if handed_off {
            ARM_GRACE
        } else {
            Duration::ZERO
        };
    while arm.try_wait()?.is_none() && Instant::now() < deadline {
        thread::sleep(Duration::from_secs(1));
    }
    stop_group(arm)
}

fn stop_group(child: &mut Child) -> Result<()> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    proc::status(Command::new("kill").args(["-TERM", "--", &format!("-{}", child.id())]))?;
    child.wait()?;
    Ok(())
}

fn disk_kb(path: &Path) -> Option<u64> {
    let output = Command::new("du").arg("-sk").arg(path).output().ok()?;
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn harbor_result(job_dir: &Path) -> Option<Value> {
    let entries = fs::read_dir(job_dir).ok()?;
    let result = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path().join("result.json"))
        .find_map(|path| read_json(&path))?;
    Some(json!({
        "task_name": result.get("task_name"),
        "reward": result.pointer("/verifier_result/rewards/reward").cloned(),
        "exception": result.pointer("/exception_info/exception_type").cloned(),
        "agent_started": result.pointer("/agent_execution/started_at").cloned(),
        "agent_finished": result.pointer("/agent_execution/finished_at").cloned(),
    }))
}
