use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;

use super::fail;
use super::json::number;

const GPU_SAMPLE: Duration = Duration::from_millis(500);

pub(crate) fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

pub(crate) fn uid() -> u32 {
    rustix::process::getuid().as_raw()
}

/// Reads a file, treating an absent or vanished one as None.
pub(crate) fn read_text(path: impl AsRef<Path>) -> Option<String> {
    match fs::read(path.as_ref()) {
        Ok(bytes) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Err(error) if matches!(error.raw_os_error(), Some(2 | 3 | 19 | 13 | 20)) => None,
        Err(error) => fail(&format!("{}: {error}", path.as_ref().display()), 1),
    }
}

fn subslice() -> String {
    env("AGENT_CAPACITY_SUBSLICE").filter(|name| !name.is_empty()).map_or_else(String::new, |name| format!("-{name}"))
}

pub(crate) fn aggregate_slice() -> String {
    format!("agent-capacity{}.slice", subslice())
}

/// Sibling of session.slice (300 unattended), app.slice (1000 attended, 100 unattended) and agent.slice (20): agents' game clients outrank batch work, yield to the desktop while attended and never outrank the compositor.
pub(crate) fn native_slice() -> String {
    format!("native{}.slice", subslice())
}

pub(crate) fn lease_slice(class: &str) -> String {
    if class == "native" { native_slice() } else { aggregate_slice() }
}

/// systemd nests a-b.slice inside a.slice.
pub(crate) fn slice_path(slice: &str) -> String {
    let parts: Vec<&str> = slice.trim_end_matches(".slice").split('-').collect();
    (1..=parts.len()).map(|count| format!("{}.slice", parts[..count].join("-"))).collect::<Vec<_>>().join("/")
}

/// The capacity lease whose scope this process runs in, read from its cgroup (`AGENT_CAPACITY_SELF_CGROUP` names another file for tests).
pub(crate) fn enclosing_lease() -> Option<String> {
    static SCOPE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)/agent-capacity-([0-9a-f]{8})([0-9a-f]{4})([0-9a-f]{4})([0-9a-f]{4})([0-9a-f]{12})\.scope(/|$)").expect("scope pattern"));
    let text = read_text(env("AGENT_CAPACITY_SELF_CGROUP").unwrap_or_else(|| "/proc/self/cgroup".to_owned()))?;
    SCOPE.captures(&text).map(|parts| (1..=5).map(|index| parts[index].to_owned()).collect::<Vec<_>>().join("-"))
}

pub(crate) fn cgroup_root() -> String {
    env("AGENT_CAPACITY_CGROUP_ROOT").unwrap_or_else(|| "/sys/fs/cgroup".to_owned())
}

pub(crate) fn host_cores() -> f64 {
    let fixture = env("AGENT_CAPACITY_CORES").map_or(0.0, |text| number(&text));
    if fixture.is_finite() && fixture != 0.0 {
        fixture
    } else {
        std::thread::available_parallelism().map_or(1.0, |count| count.get() as f64)
    }
}

pub(crate) fn user_manager_cgroup() -> PathBuf {
    let uid = uid();
    PathBuf::from(format!("{}/user.slice/user-{uid}.slice/user@{uid}.service", cgroup_root()))
}

pub(crate) fn scope_cgroup(class: &str, unit: &str) -> PathBuf {
    user_manager_cgroup().join(slice_path(&lease_slice(class))).join(unit)
}

static AVG10: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"avg10=([0-9]+(?:\.[0-9]+)?)").expect("avg10 pattern"));

fn parse_psi(path: &Path, kind: &str) -> f64 {
    let text = read_text(path).unwrap_or_else(|| fail(&format!("cannot read {kind} avg10 from {}", path.display()), 2));
    let line = text.lines().find(|line| line.starts_with(&format!("{kind} ")));
    match line.and_then(|line| AVG10.captures(line)) {
        Some(captures) => (number(&captures[1]) * 100.0).round(),
        None => fail(&format!("cannot read {kind} avg10 from {}", path.display()), 2),
    }
}

fn slice_pressure(slice: &str) -> f64 {
    let path = env("AGENT_CAPACITY_CPU_PRESSURE").map_or_else(|| user_manager_cgroup().join(slice).join("cpu.pressure"), PathBuf::from);
    if path.exists() { parse_psi(&path, "some") } else { 0.0 }
}

fn unleased_agent_pressure() -> f64 {
    let path = user_manager_cgroup().join("agent.slice");
    let aggregate = slice_path(&aggregate_slice());
    let aggregate = aggregate.split('/').nth(1).unwrap_or_default().to_owned();
    let Ok(entries) = fs::read_dir(&path) else {
        return 0.0;
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| *name != aggregate && !(name.starts_with("agent-capacity-") && name.ends_with(".scope")))
        .map(|name| slice_pressure(&format!("agent.slice/{name}")))
        .fold(0.0, f64::max)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Signals {
    pub(crate) cores: f64,
    pub(crate) memory_total_mib: f64,
    pub(crate) memory_available_mib: f64,
    pub(crate) cpu_some: f64,
    pub(crate) protected_cpu_some: f64,
    pub(crate) memory_full: f64,
}

fn meminfo() -> (f64, f64) {
    let path = env("AGENT_CAPACITY_MEMINFO").unwrap_or_else(|| "/proc/meminfo".to_owned());
    let (mut total, mut available) = (None, None);
    for line in read_text(&path).unwrap_or_default().lines() {
        let Some((name, rest)) = line.split_once(':') else { continue };
        let Some(kib) = rest.trim().strip_suffix(" kB").and_then(|kib| kib.parse::<u64>().ok()) else { continue };
        match name {
            "MemTotal" => total = Some((kib / 1024) as f64),
            "MemAvailable" => available = Some((kib / 1024) as f64),
            _ => {}
        }
    }
    total.zip(available).unwrap_or_else(|| fail("cannot read MemTotal and MemAvailable from /proc/meminfo", 2))
}

pub(crate) fn read_signals() -> Signals {
    let (memory_total_mib, memory_available_mib) = meminfo();
    let native = slice_path(&native_slice());
    let cpu_some = match env("AGENT_CAPACITY_CPU_PRESSURE") {
        Some(path) => parse_psi(Path::new(&path), "some"),
        None => [
            parse_psi(&Path::new(&cgroup_root()).join("system.slice/cpu.pressure"), "some"),
            slice_pressure("app.slice"),
            slice_pressure("background.slice"),
            slice_pressure("session.slice"),
            slice_pressure(&native),
            unleased_agent_pressure(),
        ]
        .into_iter()
        .fold(f64::MIN, f64::max),
    };
    Signals {
        cores: host_cores(),
        memory_total_mib,
        memory_available_mib,
        cpu_some,
        protected_cpu_some: slice_pressure("session.slice").max(slice_pressure(&native)),
        memory_full: parse_psi(Path::new("/proc/pressure/memory"), "full"),
    }
}

pub(crate) fn gpu_busy_path() -> Option<PathBuf> {
    if let Some(path) = env("AGENT_CAPACITY_GPU_BUSY").filter(|path| !path.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let entries = fs::read_dir("/sys/class/drm").unwrap_or_else(|error| fail(&format!("/sys/class/drm: {error}"), 1));
    entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.strip_prefix("card").is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())))
        .map(|name| PathBuf::from(format!("/sys/class/drm/{name}/device/gpu_busy_percent")))
        .find(|path| path.exists())
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Gpu {
    pub(crate) busy_percent: f64,
    pub(crate) clients: f64,
}

pub(crate) fn read_gpu(samples: u32) -> Gpu {
    let path = gpu_busy_path();
    let mut total = 0.0;
    for index in 0..samples {
        let Some(path) = &path else { break };
        if index > 0 {
            std::thread::sleep(GPU_SAMPLE);
        }
        let busy = number(read_text(path).unwrap_or_default().trim());
        total += if busy.is_nan() { 0.0 } else { busy };
    }
    let clients = env("AGENT_CAPACITY_GPU_CLIENTS").map_or_else(|| crate::capacity::warcraft::count() as f64, |text| number(&text));
    Gpu { busy_percent: if path.is_none() { 0.0 } else { (total / f64::from(samples)).round() }, clients }
}

/// The process's start time from /proc, or None once it is gone or a zombie.
pub(crate) fn process_start(pid: &str) -> Option<String> {
    let stat = read_text(format!("/proc/{pid}/stat"))?;
    let fields: Vec<&str> = stat.get(stat.rfind(')')? + 2..)?.split(' ').collect();
    if fields.first() == Some(&"Z") { None } else { fields.get(19).map(|field| (*field).to_owned()) }
}

/// /proc/self is the pid the host procfs resolves; the process id is namespace-local inside run-bounded.
pub(crate) fn wrapper_identity() -> (u64, Option<String>) {
    let pid = fs::read_link("/proc/self").ok().and_then(|link| link.file_name()?.to_str()?.parse().ok()).unwrap_or(0);
    (pid, process_start(&pid.to_string()))
}

pub(crate) fn now_ms() -> f64 {
    crate::foundation::clock::Timestamp::now().unix_millis() as f64
}

pub(crate) fn uuid() -> String {
    let mut bytes = [0u8; 16];
    let read = fs::File::open("/dev/urandom").and_then(|mut file| std::io::Read::read_exact(&mut file, &mut bytes));
    if let Err(error) = read {
        fail(&format!("/dev/urandom: {error}"), 1);
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}
