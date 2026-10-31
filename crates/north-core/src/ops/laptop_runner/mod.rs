#[cfg(test)]
mod tests;

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::fs::{FlockOperation, flock};
use rustix::process::{Pid, Signal, kill_process_group};
use serde_json::Value;

use crate::ops::agent_contain;
use crate::ops::pacing::placement::{config, gate};
use crate::platform::ssh;
use crate::threads::client::shell_quote;
use crate::platform::process::run_within;
use crate::shell::shlex_split;

const LAPTOP_WAIT: Duration = Duration::from_secs(15);
const PROBE_WAIT: Duration = Duration::from_secs(60);
const LOG_TAIL: u64 = 3000;
const STEP: Duration = Duration::from_millis(100);

pub struct Ledger {
    pub host: Option<String>,
    pub command: Vec<String>,
}

impl Ledger {
    fn threads(&self, args: &[&str]) -> Option<String> {
        let (program, fixed) = self.command.split_first()?;
        let quoted: Vec<String> = if self.host.is_some() { args.iter().map(|word| shell_quote(word)).collect() } else { args.iter().map(|word| (*word).to_owned()).collect() };
        let words: Vec<&str> = fixed.iter().chain(&quoted).map(String::as_str).collect();
        let output = run_within(program, &words, PROBE_WAIT)?;
        output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn log_name(&self, id: &str) -> String {
        self.host.as_ref().map_or_else(|| format!("queue-{id}"), |host| format!("queue-{host}-{id}"))
    }
}

fn ledgers(local: Vec<String>) -> Vec<Ledger> {
    let here = crate::platform::host::name();
    let machines = config().map(|cfg| gate::machines(&cfg)).unwrap_or_default();
    let env: std::collections::BTreeMap<String, String> = std::env::vars().collect();
    let remote = machines.into_iter().filter(|machine| machine.name != here).map(|machine| {
        let command = ssh::argv(&["ssh".into()], &env, &["-o", "BatchMode=yes", "-o", "ConnectTimeout=5", &machine.ssh, "north", "threads"]);
        Ledger { host: Some(machine.name), command: command.into_iter().map(|word| word.to_string_lossy().into_owned()).collect() }
    });
    std::iter::once(Ledger { host: None, command: local }).chain(remote).collect()
}

pub struct Inputs {
    pub placement: String,
    pub poll: Duration,
    pub runtime: PathBuf,
    pub logs: PathBuf,
    pub power_supply: PathBuf,
    pub ledgers: Vec<Ledger>,
    pub job_run: String,
    pub north: String,
    pub bun: String,
    pub agents: String,
}

impl Inputs {
    fn from_env() -> Self {
        let set = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
        let north = set("NORTH_BIN").or_else(|| std::env::current_exe().ok().map(|path| path.display().to_string())).unwrap_or_else(|| "north".into());
        Self {
            placement: set("LAPTOP_RUNNER_PLACEMENT").unwrap_or_else(|| "laptop".into()),
            poll: Duration::from_secs(set("LAPTOP_RUNNER_POLL").and_then(|poll| poll.parse().ok()).unwrap_or(30)),
            runtime: set("XDG_RUNTIME_DIR").map_or_else(|| "/tmp".into(), PathBuf::from),
            logs: set("XDG_STATE_HOME").map_or_else(|| home.join(".local/state"), PathBuf::from).join("laptop-runner"),
            power_supply: set("AGENT_LOAD_POWER_SUPPLY").map_or_else(|| "/sys/class/power_supply".into(), PathBuf::from),
            ledgers: ledgers(shlex_split(&set("LAPTOP_THREADS").unwrap_or_else(|| "threads".into())).unwrap_or_default()),
            job_run: "job-run".into(),
            north,
            bun: "bun".into(),
            agents: "agents".into(),
        }
    }

    fn laptop_state(&self) -> Value {
        let cache = self.logs.join("laptop-state.json");
        let fresh = run_within(&self.north, &["laptop", "status", "--json"], LAPTOP_WAIT)
            .filter(|output| output.status.success())
            .and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok())
            .filter(|state| !matches!(state.get("state"), None | Some(Value::Null | Value::Bool(false))));
        if let Some(state) = &fresh {
            let temporary = cache.with_extension("json.tmp");
            if fs::write(&temporary, format!("{state}\n")).is_ok() {
                let _ = fs::rename(&temporary, &cache);
            }
        }
        fresh
            .or_else(|| serde_json::from_str(&fs::read_to_string(&cache).ok()?).ok())
            .unwrap_or_else(|| serde_json::json!({"state": "unavailable", "deadline": 0}))
    }

    fn on_ac(&self) -> bool {
        let online: Vec<PathBuf> = fs::read_dir(&self.power_supply)
            .into_iter()
            .flatten()
            .flatten()
            .map(|supply| supply.path().join("online"))
            .filter(|path| path.exists())
            .collect();
        online.is_empty() || online.iter().any(|path| fs::read_to_string(path).is_ok_and(|text| text.lines().any(|line| line == "1")))
    }

    fn heavy_lease_grantable(&self) -> bool {
        let located = run_within(&self.agents, &["path", "machine-capacity"], PROBE_WAIT).filter(|output| output.status.success());
        let located = located.map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned()).unwrap_or_default();
        let base = Path::new(&located).parent().map_or_else(|| ".".into(), Path::to_path_buf);
        let script = base.join("scripts/machine-capacity.mjs");
        run_within(&self.bun, &[&script.display().to_string(), "probe", "--class", "heavy"], PROBE_WAIT).is_some_and(|output| output.status.success())
    }

    fn places(&self) -> Option<String> {
        if self.placement == "laptop" && self.laptop_state()["state"] != "available" {
            return None;
        }
        if self.on_ac() && self.heavy_lease_grantable() {
            return Some(format!("{},pool", self.placement));
        }
        Some(self.placement.clone())
    }

    fn past_grace(&self) -> bool {
        if self.placement != "laptop" {
            return false;
        }
        let Some(deadline) = self.laptop_state()["deadline"].as_i64() else {
            return false;
        };
        SystemTime::now().duration_since(UNIX_EPOCH).is_ok_and(|now| now.as_secs() as i64 >= deadline)
    }
}

pub fn run(arguments: &[String]) -> u8 {
    if !arguments.is_empty() {
        eprintln!("usage: north laptop-runner");
        return 2;
    }
    let inputs = Inputs::from_env();
    let _ = agent_contain::sweep_from_env();
    let _ = Command::new(&inputs.north).args(["private-desktop", "reap"]).status();
    let _ = Command::new(&inputs.north).args(["fleet", "waits"]).status();
    drain(&inputs)
}

fn drain(inputs: &Inputs) -> u8 {
    let lock_path = inputs.runtime.join("laptop-runner.lock");
    let Ok(lock) = OpenOptions::new().create(true).append(true).open(&lock_path) else {
        eprintln!("laptop-runner: {}: cannot open the lock", lock_path.display());
        return 1;
    };
    if flock(&lock, FlockOperation::NonBlockingLockExclusive).is_err() {
        return 0;
    }
    if let Err(error) = fs::create_dir_all(&inputs.logs) {
        eprintln!("laptop-runner: {}: {error}", inputs.logs.display());
        return 1;
    }
    let me = super::laptop::hostname();
    let mut ran = 0;
    for ledger in &inputs.ledgers {
        while let Some(places) = inputs.places() {
            let Some(job) = ledger.threads(&["queue", "--by", &me, "take", &places]).map(|text| text.trim().to_owned()).filter(|text| !text.is_empty()) else {
                break;
            };
            let Some(job) = serde_json::from_str::<Value>(&job).ok() else {
                break;
            };
            run_job(inputs, ledger, &job);
            ran += 1;
        }
    }
    println!("laptop-runner: ran {ran}");
    0
}

fn run_job(inputs: &Inputs, ledger: &Ledger, job: &Value) {
    let id = match &job["id"] {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    let command: Vec<String> = job["cmd"].as_array().into_iter().flatten().filter_map(|word| word.as_str().map(str::to_owned)).collect();
    let name = ledger.log_name(&id);
    let log = inputs.logs.join(format!("{name}.log"));
    let opened = File::create(&log).and_then(|output| Ok((output.try_clone()?, output)));
    let (errors, output) = match opened {
        Ok(files) => files,
        Err(error) => {
            let message = format!("rc=1 log={}:{}\ncannot open the log: {error}", super::laptop::hostname(), log.display());
            let _ = ledger.threads(&["queue", "msg", &id, "failed", &message]);
            return;
        }
    };
    let child = Command::new(&inputs.job_run)
        .arg(&name)
        .arg("--")
        .args(&command)
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(errors)
        .process_group(0)
        .spawn();
    let (code, stopped) = match child {
        Ok(child) => wait_for(inputs, child),
        Err(error) => {
            let _ = std::fs::write(&log, format!("{error}\n"));
            (127, false)
        }
    };
    let status = if stopped { "requeued" } else if code == 0 { "done" } else { "failed" };
    let message = format!("rc={code} log={}:{}\n{}", super::laptop::hostname(), log.display(), tail(&log));
    let _ = ledger.threads(&["queue", "msg", &id, status, &message]);
}

fn wait_for(inputs: &Inputs, mut child: Child) -> (i32, bool) {
    let group = Pid::from_raw(child.id() as i32);
    let mut stopped = false;
    'watch: loop {
        let started = Instant::now();
        while started.elapsed() < inputs.poll {
            if matches!(child.try_wait(), Ok(Some(_)) | Err(_)) {
                break 'watch;
            }
            std::thread::sleep(STEP.min(inputs.poll));
        }
        if matches!(child.try_wait(), Ok(Some(_)) | Err(_)) {
            break;
        }
        if inputs.past_grace() {
            if let Some(group) = group {
                let _ = kill_process_group(group, Signal::Term);
            }
            stopped = true;
            break;
        }
    }
    let status = child.wait();
    let code = status.map_or(1, |status| status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0)));
    (code, stopped)
}

fn tail(log: &Path) -> String {
    let Ok(mut file) = File::open(log) else {
        return String::new();
    };
    let length = file.metadata().map_or(0, |meta| meta.len());
    let _ = file.seek(SeekFrom::Start(length.saturating_sub(LOG_TAIL)));
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    String::from_utf8_lossy(&bytes).trim_end_matches('\n').to_owned()
}
