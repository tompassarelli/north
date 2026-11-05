use crate::capacity_host;

use capacity_host::{Exclusive, Host};
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::{MetadataExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

const CGROUP: &str = "/sys/fs/cgroup";
const HOLD: &str = "IFS=: read -r _ _ group < /proc/self/cgroup; printf '%s\\n' \"$group\" >\"$1.tmp\"; mv \"$1.tmp\" \"$1\"; sleep 60 & wait";
const MEMINFO: &str = "MemTotal: 98655232 kB\nMemAvailable: 71680000 kB\n";

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn systemd_user_runtime() -> Option<PathBuf> {
    let uid = fs::metadata("/proc/self").ok()?.uid();
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(|| PathBuf::from(format!("/run/user/{uid}")), PathBuf::from);
    let manager = Path::new(CGROUP).join(format!("user.slice/user-{uid}.slice/user@{uid}.service"));
    let reachable = Command::new("systemctl").args(["--user", "show-environment"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success());
    (reachable && manager.is_dir() && runtime.join("systemd").is_dir() && Path::new(CGROUP).join("cgroup.controllers").is_file()).then_some(runtime)
}

struct Live {
    host: Host,
    subslice: String,
}

impl Live {
    fn new(prefix: &str) -> Option<Self> {
        let runtime = systemd_user_runtime()?;
        let host = Host::new(prefix, &[]);
        symlink(runtime.join("systemd"), host.path("systemd")).unwrap();
        let _ = fs::remove_file(host.path("agent-capacity-v1/mode"));
        fs::write(host.path("meminfo"), MEMINFO).unwrap();
        let subslice = format!("test{}x{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed));
        Some(Self { host, subslice })
    }

    fn pressure(&self, avg10: &str) {
        fs::write(self.host.path("pressure"), format!("some avg10={avg10} avg60=0.00 avg300=0.00 total=0\n")).unwrap();
    }

    fn command(&self, arguments: &[&str]) -> Command {
        let mut command = self.host.helper(arguments);
        command.env_remove("AGENT_CAPACITY_CGROUP_ROOT").env("AGENT_CAPACITY_SUBSLICE", &self.subslice).env("AGENT_CAPACITY_SETTLE_SECONDS", "3");
        command
    }

    fn json(&self, environment: &[(&str, &str)], arguments: &[&str]) -> Value {
        let mut command = self.command(arguments);
        command.envs(environment.iter().copied());
        let output = command.output_exclusive().unwrap();
        serde_json::from_slice(&output.stdout).unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stderr)))
    }

    fn probe(&self, class: &[&str]) -> Value {
        self.json(&[], &[&["probe", "--class"][..], class].concat())
    }

    fn reason(&self, environment: &[(&str, &str)], class: &[&str]) -> String {
        let value = self.json(environment, &[&["probe", "--class"][..], class].concat());
        value.get("reason").or_else(|| value.get("decision")).and_then(Value::as_str).unwrap_or_default().to_owned()
    }

    fn held(&self, name: &str, lifetime: &[&str], class: &str, owner: &str, extra: &[&str]) -> Run {
        let ready = self.host.path(&format!("{name}.ready"));
        let stderr = self.host.path(&format!("{name}.stderr"));
        let owner = format!("fixture:/{owner}");
        let ready_path = ready.to_str().unwrap().to_owned();
        let mut arguments: Vec<&str> = lifetime.to_vec();
        arguments.extend(["--class", class, "--owner", &owner]);
        arguments.extend(extra);
        arguments.extend(["--", "bash", "-c", HOLD, "fixture-hold", &ready_path]);
        let mut command = self.command(&arguments);
        command.stdout(Stdio::null()).stderr(fs::File::create(&stderr).unwrap());
        Run { child: command.spawn_exclusive().unwrap(), ready, stderr }
    }

    fn wait(&self, what: &str, done: impl FnMut(&Host) -> bool) {
        self.host.until(what, done);
    }

    fn run_until_exit(&self, arguments: &[&str]) -> (Option<i32>, Vec<Value>) {
        let stderr = self.host.path("exit.stderr");
        let mut command = self.command(arguments);
        command.stdout(Stdio::null()).stderr(fs::File::create(&stderr).unwrap());
        let status = command.status_exclusive().unwrap();
        (status.code(), events(&stderr))
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let slices = ["agent-capacity", "agent-batch", "native"].map(|prefix| format!("{prefix}-{}.slice", self.subslice));
        let _ = Command::new("systemctl").args(["--user", "stop"]).args(slices).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

struct Run {
    child: Child,
    ready: PathBuf,
    stderr: PathBuf,
}

impl Run {
    fn started(&self) -> bool {
        fs::read_to_string(&self.ready).is_ok_and(|text| text.ends_with('\n'))
    }

    fn group(&self, live: &Live) -> String {
        live.wait("a held run to start", |_| self.started());
        fs::read_to_string(&self.ready).unwrap().trim().to_owned()
    }

    fn events(&self) -> Vec<Value> {
        events(&self.stderr)
    }

    fn event(&self, live: &Live, index: usize) -> Value {
        live.wait("a decision line", |_| self.events().len() > index);
        self.events()[index].clone()
    }

    fn stop(mut self, live: &Live) -> Vec<Value> {
        assert!(Command::new("kill").args(["-TERM", &self.child.id().to_string()]).status().unwrap().success());
        live.wait("the held run to exit", |_| self.child.try_wait().unwrap().is_some());
        self.events()
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = Command::new("kill").args(["-TERM", &self.child.id().to_string()]).status();
        }
        let _ = self.child.wait();
    }
}

fn events(stderr: &Path) -> Vec<Value> {
    fs::read_to_string(stderr).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

fn decision(events: &[Value], index: usize) -> String {
    events[index]["decision"].as_str().unwrap_or_default().to_owned()
}

fn released(events: &[Value]) -> bool {
    events.last().is_some_and(|last| last["decision"] == "RELEASED")
}

fn cgroup_file(group: &str, name: &str) -> Option<String> {
    fs::read_to_string(format!("{CGROUP}{group}/{name}")).ok().map(|text| text.trim().to_owned())
}

fn parent(group: &str) -> &str {
    group.rsplit_once('/').map_or("", |(parent, _)| parent)
}

fn unlimited(group: &str) -> bool {
    cgroup_file(group, "cpu.max").is_none_or(|text| text.split(' ').next() == Some("max"))
}

fn gone(group: &str) -> bool {
    !Path::new(&format!("{CGROUP}{group}")).exists()
}

fn command_lines(group: &str) -> String {
    cgroup_file(group, "cgroup.procs")
        .unwrap_or_default()
        .lines()
        .filter_map(|pid| fs::read(format!("/proc/{pid}/cmdline")).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).replace('\0', " "))
        .collect::<Vec<_>>()
        .join(" ")
}

fn runtime_max(group: &str) -> String {
    let unit = group.rsplit('/').next().unwrap();
    let output = Command::new("systemctl").args(["--user", "show", unit, "--property=RuntimeMaxUSec", "--value"]).output().unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn seed_usage(live: &Live, class: &str, cores: f64, memory: u64, wall: u64, owner: &str) {
    let lines: String = (1..=3)
        .map(|sample| json!({"at": format!("2026-10-10T00:00:0{sample}Z"), "owner": owner, "shape": "read", "class": class, "meanCores": cores, "p90Cores": cores.max(2.0).max(if class == "heavy" { 6.0 } else { 0.0 }), "peakCores": cores, "peakMemoryMiB": memory, "wallSeconds": wall}).to_string() + "\n")
        .collect();
    fs::write(live.host.path("usage.jsonl"), lines).unwrap();
}

macro_rules! live {
    ($name:literal) => {
        match Live::new($name) {
            Some(live) => live,
            None => {
                eprintln!("skipped: no systemd user manager with delegated cgroup v2");
                return;
            }
        }
    };
}

#[test]
fn a_run_past_its_timeout_has_its_scope_and_every_descendant_killed() {
    let live = live!("capacity-live-timeout-");
    let pid_file = live.host.path("sleeper.pid");
    let script = "sleep 60 & child=$!; printf '%s\\n' \"$child\" >\"$1\"; wait \"$child\"";
    let (code, _) = live.run_until_exit(&["run", "--class", "heavy", "--owner", "fixture:/capacity", "--timeout-seconds", "1", "--", "bash", "-c", script, "fixture-scope", pid_file.to_str().unwrap()]);
    assert_ne!(code, Some(0));
    let pid = fs::read_to_string(&pid_file).unwrap().trim().to_owned();
    assert!(pid.parse::<u32>().is_ok_and(|pid| pid > 0), "{pid}");
    live.wait("the descendant to stop", |_| !Path::new(&format!("/proc/{pid}")).exists());
}

#[test]
fn heavy_jobs_share_the_idle_weighted_batch_slice_unthrottled_and_a_fourth_waits_for_the_settle_window() {
    let live = live!("capacity-live-aggregate-");
    seed_usage(&live, "heavy", 4.8, 6826, 120, "fixture:/capacity");
    let mut runs = Vec::new();
    for number in 1..=3 {
        let run = live.held(&format!("group-{number}"), &["run"], "heavy", "capacity", &["--timeout-seconds", "30"]);
        run.group(&live);
        runs.push(run);
    }
    let fourth = live.held("group-4", &["run"], "heavy", "capacity", &["--timeout-seconds", "30"]);
    let queued = fourth.event(&live, 0);
    let groups: Vec<String> = runs.iter().map(|run| run.group(&live)).collect();
    for group in &groups {
        let batch = parent(group);
        assert!(batch.ends_with(&format!("/agent.slice/agent-batch.slice/agent-batch-{}.slice", live.subslice)), "{batch}");
        assert_eq!(cgroup_file(batch, "cpu.idle").as_deref(), Some("1"), "{batch}");
        assert!(unlimited(batch) && unlimited(group), "{group}");
        assert_eq!(cgroup_file(group, "memory.high"), Some((8192u64 * 1024 * 1024).to_string()));
    }
    assert!(!fourth.started());
    assert_eq!((queued["decision"].as_str(), queued["reason"].as_str()), (Some("QUEUED"), Some("DEFER_CPU_PRESSURE")), "{queued}");
    let probe = live.probe(&["exclusive"]);
    assert_eq!(probe["reason"], "DEFER_EXCLUSIVE", "{probe}");
    assert_eq!((probe["leasedCpuCeilings"].as_u64(), probe["queuedAhead"].as_u64(), probe["aggregateCpuLimit"].as_u64(), probe["profile"].as_str()), (Some(18), Some(1), Some(20), Some("attended")), "{probe}");
    fourth.group(&live);
    let finished: Vec<Vec<Value>> = runs.into_iter().map(|run| run.stop(&live)).collect();
    let last = fourth.stop(&live);
    for events in finished.iter().chain([&last]) {
        assert!(released(events), "{events:?}");
    }
    live.wait("every cgroup to be removed", |_| groups.iter().all(|group| gone(group)));
    assert_eq!(live.probe(&["exclusive"])["decision"], "RUN");
    let clients = [("AGENT_CAPACITY_GPU_CLIENTS", "4")];
    let both = (live.reason(&clients, &["native", "--memory-gib", "1"]), live.reason(&clients, &["native", "--new-clients", "0", "--memory-gib", "1"]));
    assert_eq!((both.0.as_str(), both.1.as_str()), ("DEFER_GPU_CLIENTS", "RUN"));
}

#[test]
fn an_agent_allowance_does_not_block_its_exclusive_command_which_then_blocks_peers() {
    let live = live!("capacity-live-exclusive-");
    let lease = live.json(&[], &["reserve", "--class", "agent", "--owner", "fixture:/capacity", "--timeout-seconds", "30"])["lease"].as_str().unwrap().to_owned();
    let exclusive = live.held("exclusive", &["run"], "exclusive", "capacity", &["--timeout-seconds", "30"]);
    exclusive.group(&live);
    assert_eq!(live.reason(&[], &["moderate"]), "DEFER_EXCLUSIVE");
    assert!(released(&exclusive.stop(&live)));
    let released_lease = live.json(&[], &["release", "--lease", &lease, "--owner", "fixture:/capacity"]);
    assert_eq!(released_lease["decision"], "RELEASED");
}

#[test]
fn pressure_holds_a_run_in_the_queue_until_it_eases_and_leaving_the_queue_drops_the_ticket() {
    let live = live!("capacity-live-pressure-");
    live.pressure("90.00");
    let held = live.held("held", &["run"], "moderate", "capacity", &["--timeout-seconds", "30"]);
    assert_eq!(decision(&[held.event(&live, 0)], 0), "QUEUED");
    assert!(!held.started());
    assert_eq!(live.probe(&["agent"])["queuedAhead"], 1);
    live.pressure("0.00");
    held.group(&live);
    assert!(released(&held.stop(&live)));
    live.pressure("90.00");
    let queued = live.host.path("agent-capacity-v1/queue");
    let waiting = live.held("cancel", &["run"], "heavy", "capacity", &["--timeout-seconds", "30"]);
    waiting.event(&live, 0);
    assert_eq!(fs::read_dir(&queued).unwrap().count(), 1);
    drop(waiting.stop(&live));
    live.wait("the ticket to drop", |_| fs::read_dir(&queued).unwrap().count() == 0);
}

#[test]
fn a_batch_session_takes_its_class_deadline_and_keeps_its_allowance_until_its_scope_stops() {
    let live = live!("capacity-live-session-");
    let child = live.host.path("session.child");
    let script = "IFS=: read -r _ _ group < /proc/self/cgroup; printf '%s\\n' \"$group\" >\"$1.tmp\"; mv \"$1.tmp\" \"$1\"; sleep 60 & printf '%s\\n' \"$!\" >\"$2\"; wait";
    let ready = live.host.path("session.ready");
    let stderr = live.host.path("session.stderr");
    let mut command = live.command(&["session", "--class", "moderate", "--owner", "fixture:/capacity", "--", "bash", "-c", script, "fixture-session", ready.to_str().unwrap(), child.to_str().unwrap()]);
    command.stdout(Stdio::null()).stderr(fs::File::create(&stderr).unwrap());
    let run = Run { child: command.spawn_exclusive().unwrap(), ready, stderr };
    let group = run.group(&live);
    live.wait("the child pid", |_| fs::read_to_string(&child).is_ok_and(|text| text.ends_with('\n')));
    let session_child = fs::read_to_string(&child).unwrap().trim().to_owned();
    let lines = command_lines(&group);
    assert_eq!(runtime_max(&group), "infinity");
    assert!(lines.contains("capacity deadline ") && lines.contains(" 1800 --"), "{lines}");
    assert!(!run.event(&live, 0)["expiresAt"].is_null());
    let probe = live.probe(&["exclusive"]);
    assert_eq!((probe["reason"].as_str(), probe["leasedMemoryMiB"].as_u64(), probe["reclaimed"].as_u64()), (Some("DEFER_EXCLUSIVE"), Some(2048), Some(0)), "{probe}");
    assert!(released(&run.stop(&live)));
    live.wait("the session cgroup to be removed", |_| gone(&group));
    assert!(!Path::new(&format!("/proc/{session_child}")).exists());
    let (code, events) = live.run_until_exit(&["session", "--class", "moderate", "--owner", "fixture:/capacity", "--", "bash", "-c", "exit 7"]);
    assert_eq!(code, Some(7));
    assert!(released(&events), "{events:?}");
    assert_eq!(live.probe(&["agent"])["leasedMemoryMiB"], 0);
}

#[test]
fn a_native_client_runs_unthrottled_at_high_weight_beside_the_batch_slice() {
    let live = live!("capacity-live-native-");
    let native = live.held("native", &["run"], "native", "capacity", &["--timeout-seconds", "30"]);
    let group = native.group(&live);
    let slice = parent(&group);
    assert!(slice.ends_with(&format!("/native.slice/native-{}.slice", live.subslice)), "{slice}");
    assert_eq!(cgroup_file(slice, "cpu.weight").as_deref(), Some("200"));
    assert!(unlimited(&group));
    assert_eq!(cgroup_file(&group, "memory.high"), Some((4096u64 * 1024 * 1024).to_string()));
    let probe = live.probe(&["heavy"]);
    assert_eq!((probe["decision"].as_str(), probe["leasedNativeCpus"].as_u64()), (Some("RUN"), Some(2)), "{probe}");
    assert!(released(&native.stop(&live)));
}

#[test]
fn a_memory_request_reaches_the_lease_accounting_and_the_kernel_for_runs_and_sessions() {
    let live = live!("capacity-live-memory-");
    for lifetime in ["run", "session"] {
        let extra: &[&str] = if lifetime == "run" { &["--timeout-seconds", "30"] } else { &[] };
        let held = live.held(&format!("memory-{lifetime}"), &[lifetime], "native", "capacity", &[&["--memory-gib", "1.5"][..], extra].concat());
        let group = held.group(&live);
        assert_eq!(cgroup_file(&group, "memory.high"), Some("1610612736".to_owned()));
        assert!(lifetime == "run" || runtime_max(&group) == "infinity");
        let first = held.event(&live, 0);
        assert_eq!(first["requestedMemoryMiB"], 1536, "{first}");
        let lease: Value = serde_json::from_str(&fs::read_to_string(live.host.path(&format!("agent-capacity-v1/leases/{}.json", first["lease"].as_str().unwrap()))).unwrap()).unwrap();
        assert_eq!(lease["memoryMiB"], 1536);
        let probe = live.probe(&["native", "--memory-gib", "1.5"]);
        assert_eq!((probe["leasedMemoryMiB"].as_u64(), probe["requestedMemoryMiB"].as_u64()), (Some(1536), Some(1536)), "{probe}");
        assert!(released(&held.stop(&live)));
    }
}

#[test]
fn an_exclusive_request_goes_first_and_later_heavy_work_waits_behind_it_while_a_moderate_build_still_starts() {
    let live = live!("capacity-live-drain-");
    seed_usage(&live, "moderate", 2.0, 512, 120, "fixture:/seed");
    live.pressure("30.00");
    let early = live.held("early", &["run"], "moderate", "early", &["--timeout-seconds", "30"]);
    assert_eq!(early.event(&live, 0)["reason"], "DEFER_INTERACTIVE_PRESSURE");
    let drain = live.held("drain", &["session"], "exclusive", "drain", &[]);
    let group = drain.group(&live);
    let lines = command_lines(&group);
    assert_eq!(runtime_max(&group), "infinity");
    assert!(lines.contains("capacity deadline ") && lines.contains(" 900 --"), "{lines}");
    live.pressure("0.00");
    assert_eq!(early.event(&live, 1)["reason"], "DEFER_EXCLUSIVE");
    assert!(!early.started());
    let status = live.json(&[], &["status"]);
    let owners: Vec<&str> = status["holding"].as_array().unwrap().iter().chain(status["queued"].as_array().unwrap()).map(|entry| entry["owner"].as_str().unwrap()).collect();
    assert_eq!((owners, status["queued"][0]["position"].as_u64()), (vec!["fixture:/drain", "fixture:/early"], Some(1)), "{status}");
    assert!(released(&drain.stop(&live)));
    early.group(&live);

    let second = live.held("second", &["run"], "exclusive", "drain", &["--timeout-seconds", "30"]);
    second.event(&live, 0);
    let build = live.held("build", &["run"], "moderate", "build", &["--timeout-seconds", "30"]);
    build.group(&live);
    let landing = live.command(&["run", "--class", "moderate", "--owner", "land-check", "--timeout-seconds", "30", "--", "true"]).env("AGENT_CAPACITY_CORES", "4").env("SAFE_PUSH_LAND_HEAD", "0123456").stdout(Stdio::null()).stderr(Stdio::null()).status_exclusive().unwrap();
    assert!(landing.success());
    let saturated = live.host.path("saturated.stderr");
    let mut command = live.command(&["run", "--class", "moderate", "--owner", "land-check", "--timeout-seconds", "30", "--", "true"]);
    command.env("AGENT_CAPACITY_CORES", "4").stdout(Stdio::null()).stderr(fs::File::create(&saturated).unwrap());
    let mut waiting = command.spawn_exclusive().unwrap();
    live.wait("the saturated run to be refused a place", |_| events(&saturated).first().is_some_and(|event| event["reason"] == "DEFER_BATCH_JOBS"));
    waiting.kill().unwrap();
    waiting.wait().unwrap();
    let late = live.held("late", &["run"], "heavy", "late", &["--timeout-seconds", "30"]);
    late.event(&live, 0);
    assert_eq!(second.events()[0]["reason"], "DEFER_EXCLUSIVE");
    assert_eq!(late.events()[0]["reason"], "DEFER_EXCLUSIVE_QUEUED");
    let status = live.json(&[], &["status"]);
    let holding: Vec<String> = status["holding"].as_array().unwrap().iter().map(|entry| format!("{} {}", entry["owner"].as_str().unwrap(), entry["class"].as_str().unwrap())).collect();
    let queued: Vec<&str> = status["queued"].as_array().unwrap().iter().map(|entry| entry["class"].as_str().unwrap()).collect();
    assert_eq!((holding, queued), (vec!["fixture:/early moderate".to_owned(), "fixture:/build moderate".to_owned()], vec!["exclusive", "heavy"]), "{status}");
    assert!(status["holding"][0]["remainingSeconds"].as_f64().unwrap() <= 35.0);
    assert!(released(&early.stop(&live)));
    assert!(released(&build.stop(&live)));
    second.group(&live);
    assert_eq!(late.event(&live, 1)["reason"], "DEFER_EXCLUSIVE");
    assert!(!late.started());
    assert!(released(&second.stop(&live)));
    late.group(&live);
    assert!(released(&late.stop(&live)));
    let end = live.json(&[], &["status"]);
    assert_eq!((end["holding"].as_array().unwrap().len(), end["queued"].as_array().unwrap().len()), (0, 0), "{end}");
}
