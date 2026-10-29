use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use super::gpu::add_engine_time;
use super::json::{Obj, num, number, opt};
use super::machine::{now_ms, read_text};
use super::policy::{disk_busy_share, percentile, quiet_idle_cores};
use super::state::write_private;

const SAMPLE: Duration = Duration::from_millis(1000);
const PEAK_WINDOW_MILLISECONDS: f64 = 5000.0;
const LIVE_WINDOW_MILLISECONDS: f64 = 120_000.0;
const GPU_EVERY_SAMPLES: u64 = 5;
const MIB: f64 = 1_048_576.0;

/// What the lease scope used while it ran.
#[derive(Clone, Debug, Default)]
pub(crate) struct Usage {
    pub(crate) cpu_seconds: f64,
    pub(crate) peak_cores: f64,
    pub(crate) peak_memory_mib: f64,
    pub(crate) gpu_seconds: f64,
    pub(crate) io_read_bytes: Option<f64>,
    pub(crate) io_write_bytes: Option<f64>,
    pub(crate) io_pressure_some_usec: Option<f64>,
    pub(crate) disk_busy_share: Option<f64>,
    pub(crate) p90_cores: Option<f64>,
    pub(crate) wall_seconds: f64,
}

pub(crate) fn physical_disk_busy_milliseconds() -> Vec<(String, f64)> {
    read_text("/proc/diskstats")
        .unwrap_or_default()
        .trim()
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let disk = *fields.get(2)?;
            let ticks = number(fields.get(12).copied().unwrap_or("x"));
            (Path::new(&format!("/sys/block/{disk}/device")).exists() && ticks.is_finite()).then(|| (disk.to_owned(), ticks))
        })
        .collect()
}

fn summed(stat: &str, key: &str) -> f64 {
    stat.split(|character: char| !(character.is_ascii_alphanumeric() || character == '=' || character == '_'))
        .filter_map(|word| word.strip_prefix(key)?.strip_prefix('=')?.parse::<u64>().ok())
        .map(|value| value as f64)
        .sum()
}

struct Sampler {
    root: PathBuf,
    lease: String,
    cgroup: PathBuf,
    started_at: f64,
    count: u64,
    samples: VecDeque<(f64, f64)>,
    live: VecDeque<(f64, f64)>,
    recent: Vec<f64>,
    per_client: BTreeMap<String, f64>,
    usage: Usage,
    view: Obj,
}

impl Sampler {
    fn io(&mut self) {
        if let Some(stat) = read_text(self.cgroup.join("io.stat")) {
            self.usage.io_read_bytes = Some(summed(&stat, "rbytes"));
            self.usage.io_write_bytes = Some(summed(&stat, "wbytes"));
        }
        let pressure = read_text(self.cgroup.join("io.pressure")).and_then(|text| {
            let line = text.lines().find(|line| line.starts_with("some "))?;
            line.split_whitespace().find_map(|word| word.strip_prefix("total=")?.parse::<u64>().ok())
        });
        if let Some(total) = pressure {
            self.usage.io_pressure_some_usec = Some(total as f64);
        }
    }

    fn cores(&mut self, now: f64, usec: f64) {
        self.samples.push_back((now, usec));
        self.live.push_back((now, usec));
        while self.live.len() > 2 && now - self.live[1].0 >= LIVE_WINDOW_MILLISECONDS {
            self.live.pop_front();
        }
        let (live_then, live_before) = self.live[0];
        self.view.set("liveCores", if now > live_then { num((usec - live_before) / 1000.0 / (now - live_then)) } else { serde_json::Value::Null });
        self.view.set("liveWindowSeconds", num((now - live_then) / 1000.0));
        while self.samples.len() > 2 && now - self.samples[1].0 >= PEAK_WINDOW_MILLISECONDS {
            self.samples.pop_front();
        }
        let (then, before) = self.samples[0];
        if now - then >= PEAK_WINDOW_MILLISECONDS / 2.0 {
            let recent = (usec - before) / 1000.0 / (now - then);
            self.view.set("recentCores", num(recent));
            if recent >= quiet_idle_cores() {
                self.view.set("lastBusyAt", num(now));
            }
            self.usage.peak_cores = self.usage.peak_cores.max(recent);
            self.recent.push(recent);
        }
    }

    fn memory(&mut self) {
        let read = |name: &str| read_text(self.cgroup.join(name)).map_or(f64::NAN, |text| number(text.trim()));
        let (peak, current) = (read("memory.peak"), read("memory.current"));
        if peak.is_finite() {
            self.usage.peak_memory_mib = self.usage.peak_memory_mib.max(peak / MIB);
        }
        if current.is_finite() {
            self.view.set("memoryMiB", num(current / MIB));
        }
    }

    fn sample(&mut self) {
        let now = now_ms();
        self.io();
        let usec = read_text(self.cgroup.join("cpu.stat"))
            .and_then(|stat| stat.lines().find_map(|line| line.strip_prefix("usage_usec ")?.parse::<u64>().ok()));
        let Some(usec) = usec.map(|usec| usec as f64) else { return };
        self.cores(now, usec);
        self.usage.cpu_seconds = usec / 1e6;
        self.memory();
        if self.count.is_multiple_of(GPU_EVERY_SAMPLES) {
            let pids: Vec<String> = read_text(self.cgroup.join("cgroup.procs")).unwrap_or_default().lines().filter(|line| !line.is_empty()).map(str::to_owned).collect();
            add_engine_time(&pids, &mut self.per_client);
            self.usage.gpu_seconds = self.per_client.values().sum::<f64>() / 1e9;
        }
        self.count += 1;
        self.write(now);
    }

    fn write(&mut self, now: f64) {
        let mean = self.usage.cpu_seconds / ((now - self.started_at) / 1000.0).max(1.0);
        let view = self
            .view
            .clone()
            .put("cpuSeconds", num(self.usage.cpu_seconds))
            .put("meanCores", num(mean))
            .put("peakCores", num(self.usage.peak_cores))
            .put("peakMemoryMiB", num(self.usage.peak_memory_mib))
            .put("gpuSeconds", num(self.usage.gpu_seconds))
            .put("ioReadBytes", opt(self.usage.io_read_bytes))
            .put("ioWriteBytes", opt(self.usage.io_write_bytes))
            .put("ioPressureSomeUsec", opt(self.usage.io_pressure_some_usec))
            .put("at", num(now));
        let path = self.root.join("leases").join(format!("{}.usage", self.lease));
        if let Err(error) = write_private(&path, &format!("{}\n", view.text()), false)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("machine-capacity: {}: {error}", path.display());
        }
    }
}

pub(crate) struct Measurement {
    stop: mpsc::Sender<()>,
    worker: JoinHandle<Usage>,
}

/// Samples the lease scope every second while it runs; the scope's cgroup disappears with its last process.
pub(crate) fn measure_scope(root: &Path, lease: &str, cgroup: PathBuf) -> Measurement {
    let started_at = now_ms();
    let view = Obj::new().put("recentCores", 0).put("lastBusyAt", num(started_at)).put("memoryMiB", 0);
    let mut sampler = Sampler {
        root: root.to_owned(),
        lease: lease.to_owned(),
        cgroup,
        started_at,
        count: 0,
        samples: VecDeque::new(),
        live: VecDeque::new(),
        recent: Vec::new(),
        per_client: BTreeMap::new(),
        usage: Usage::default(),
        view,
    };
    let disks_before = physical_disk_busy_milliseconds();
    let (stop, stopped) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(SAMPLE) {
            sampler.sample();
        }
        sampler.sample();
        let wall = now_ms() - sampler.started_at;
        let mut usage = sampler.usage.clone();
        usage.disk_busy_share = disk_busy_share(&disks_before, &physical_disk_busy_milliseconds(), wall);
        usage.p90_cores = (!sampler.recent.is_empty()).then(|| percentile(&sampler.recent, 0.9));
        usage.wall_seconds = wall / 1000.0;
        usage
    });
    Measurement { stop, worker }
}

impl Measurement {
    pub(crate) fn finish(self) -> Usage {
        let _ = self.stop.send(());
        self.worker.join().unwrap_or_default()
    }
}
