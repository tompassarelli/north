use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use super::json::{num, round2};
use super::machine::{env, gpu_busy_path, now_ms, process_start, read_text};
use super::shape::recorded_command;

pub struct DrmClient {
    pub pid: String,
    pub pdev: Option<String>,
    pub client: String,
    pub nanoseconds: f64,
}

fn line_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    text.lines().find_map(|line| line.strip_prefix(key)?.strip_prefix(':')).map(str::trim)
}

fn engine_nanoseconds(text: &str) -> f64 {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            let engine = key.strip_prefix("drm-engine-")?;
            if engine.is_empty() || !engine.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_') {
                return None;
            }
            value.trim().strip_suffix(" ns")?.parse::<u64>().ok()
        })
        .map(|nanoseconds| nanoseconds as f64)
        .sum()
}

/// Per-process GPU engine time from amdgpu fdinfo, one entry per open DRM client file.
pub fn drm_clients(pids: &[String]) -> Vec<DrmClient> {
    let mut clients = Vec::new();
    for pid in pids {
        let Ok(entries) = fs::read_dir(format!("/proc/{pid}/fdinfo")) else { continue };
        for entry in entries.flatten() {
            let Some(text) = read_text(entry.path()) else { continue };
            if !text.contains("drm-driver:\tamdgpu") {
                continue;
            }
            let Some(client) = line_value(&text, "drm-client-id").filter(|id| !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())) else { continue };
            let pdev = line_value(&text, "drm-pdev").filter(|pdev| !pdev.is_empty() && !pdev.contains(char::is_whitespace)).map(str::to_owned);
            clients.push(DrmClient { pid: pid.clone(), pdev, client: client.to_owned(), nanoseconds: engine_nanoseconds(&text) });
        }
    }
    clients
}

/// The GPU seconds the processes' DRM clients used, each client counted once at its largest total.
pub fn add_engine_time(pids: &[String], per_client: &mut BTreeMap<String, f64>) {
    for client in drm_clients(pids) {
        let total = per_client.entry(client.client).or_insert(0.0);
        *total = total.max(client.nanoseconds);
    }
}

#[derive(Clone, Debug)]
pub struct GpuClient {
    client: String,
    pid: u64,
    start: String,
    command: String,
    cgroup: String,
    nanoseconds: f64,
}

pub struct GpuProcessSample {
    pub at: f64,
    clients: Vec<GpuClient>,
}

fn process_command(pid: &str) -> String {
    let mut words: Vec<String> = read_text(format!("/proc/{pid}/cmdline")).unwrap_or_default().split('\0').filter(|word| !word.is_empty()).map(str::to_owned).collect();
    if let Some(first) = words.first_mut() {
        *first = first.rsplit('/').next().unwrap_or_default().to_owned();
    }
    let command: String = recorded_command(&words).join(" ").chars().take(120).collect();
    if command.is_empty() { read_text(format!("/proc/{pid}/comm")).unwrap_or_default().trim().to_owned() } else { command }
}

fn process_identity(pid: &str) -> Option<(String, String, String)> {
    let start = process_start(pid)?;
    let cgroup = read_text(format!("/proc/{pid}/cgroup"))?.lines().find_map(|line| line.strip_prefix("0::").map(str::to_owned))?;
    Some((start, process_command(pid), cgroup))
}

pub fn take_gpu_process_sample() -> GpuProcessSample {
    let device = gpu_busy_path()
        .filter(|_| env("AGENT_CAPACITY_GPU_BUSY").is_none())
        .and_then(|path| fs::canonicalize(path.parent().unwrap_or(Path::new("/"))).ok())
        .and_then(|device| Some(device.file_name()?.to_string_lossy().into_owned()));
    let pids: Vec<String> = fs::read_dir("/proc")
        .map(|entries| entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).filter(|name| name.bytes().all(|byte| byte.is_ascii_digit())).collect())
        .unwrap_or_default();
    let mut identities: BTreeMap<String, Option<(String, String, String)>> = BTreeMap::new();
    let mut clients = Vec::new();
    for drm in drm_clients(&pids) {
        if device.is_some() && drm.pdev != device {
            continue;
        }
        let identity = identities.entry(drm.pid.clone()).or_insert_with(|| process_identity(&drm.pid));
        if let Some((start, command, cgroup)) = identity {
            let client = format!("{}:{}", drm.pdev.as_deref().unwrap_or_default(), drm.client);
            clients.push(GpuClient { client, pid: drm.pid.parse().unwrap_or(0), start: start.clone(), command: command.clone(), cgroup: cgroup.clone(), nanoseconds: drm.nanoseconds });
        }
    }
    GpuProcessSample { at: now_ms(), clients }
}

/// The unleased process that used the most GPU time between the samples.
pub fn top_unleased_gpu_process(before: &GpuProcessSample, after: &GpuProcessSample, leased_cgroups: &[String]) -> Value {
    if after.at <= before.at {
        return Value::Null;
    }
    let leased = |client: &GpuClient| leased_cgroups.iter().any(|group| client.cgroup == *group || client.cgroup.starts_with(&format!("{group}/")));
    let leased_clients: Vec<&str> = after.clients.iter().filter(|client| leased(client)).map(|client| client.client.as_str()).collect();
    let mut latest: Vec<&GpuClient> = Vec::new();
    for client in after.clients.iter().filter(|client| !leased_clients.contains(&client.client.as_str())) {
        match latest.iter_mut().find(|seen| seen.client == client.client) {
            Some(seen) if client.nanoseconds > seen.nanoseconds => *seen = client,
            Some(_) => {}
            None => latest.push(client),
        }
    }
    let mut processes: Vec<(u64, String, f64)> = Vec::new();
    for client in latest {
        let Some(older) = before.clients.iter().rev().find(|older| older.client == client.client) else { continue };
        if older.pid != client.pid || older.start != client.start {
            continue;
        }
        let delta = (client.nanoseconds - older.nanoseconds).max(0.0);
        match processes.iter_mut().find(|(pid, _, _)| *pid == client.pid) {
            Some(process) => process.2 += delta,
            None => processes.push((client.pid, client.command.clone(), delta)),
        }
    }
    processes.retain(|(_, _, nanoseconds)| *nanoseconds > 0.0);
    processes.sort_by(|left, right| right.2.total_cmp(&left.2).then(left.0.cmp(&right.0)));
    processes.first().map_or(Value::Null, |(pid, command, nanoseconds)| {
        let busy = (nanoseconds / ((after.at - before.at) * 1e6) * 100.0).min(100.0);
        json!({ "pid": pid, "command": command, "gpuBusyPercent": num(round2(busy)) })
    })
}

/// NVML offers no per-process engine counter without accounting mode (root), so `nvidia-smi pmon` utilization stands in for the DRM engine time nvidia's driver does not expose.
pub fn pmon_busy_fractions(text: &str, pids: &[String]) -> f64 {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 9 || !pids.iter().any(|pid| pid == fields[1]) {
                return None;
            }
            let busiest = fields[3..9].iter().enumerate().filter(|(index, _)| *index != 1).filter_map(|(_, field)| field.parse::<f64>().ok()).fold(0.0_f64, f64::max);
            Some((fields[1], busiest.clamp(0.0, 100.0) / 100.0))
        })
        .fold(BTreeMap::<&str, f64>::new(), |mut busiest, (pid, fraction)| {
            let entry = busiest.entry(pid).or_insert(0.0);
            *entry = entry.max(fraction);
            busiest
        })
        .values()
        .sum()
}

fn has_nvidia_device_open(pid: &str) -> bool {
    fs::read_dir(format!("/proc/{pid}/fd")).is_ok_and(|entries| {
        entries.flatten().any(|entry| fs::read_link(entry.path()).is_ok_and(|target| target.to_string_lossy().starts_with("/dev/nvidia")))
    })
}

/// The share of one GPU the processes kept busy right now, summed over processes; zero when none has an nvidia device open or nvidia-smi is missing.
pub fn nvidia_busy_fractions(pids: &[String]) -> f64 {
    let holders: Vec<String> = pids.iter().filter(|pid| has_nvidia_device_open(pid)).cloned().collect();
    if holders.is_empty() {
        return 0.0;
    }
    let program = env("AGENT_CAPACITY_NVIDIA_SMI").unwrap_or_else(|| "nvidia-smi".to_owned());
    let output = Command::new(program).args(["pmon", "-c", "1"]).stdin(Stdio::null()).stderr(Stdio::null()).output();
    match output {
        Ok(output) if output.status.success() => pmon_busy_fractions(&String::from_utf8_lossy(&output.stdout), &holders),
        _ => 0.0,
    }
}
