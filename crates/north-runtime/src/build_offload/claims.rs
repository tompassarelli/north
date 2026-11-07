use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// One build this process sends to a host, kept as a file holding the cores it asks for, so builds asking at the same moment count each other before the host's load shows them.
pub(super) struct Claim {
    path: PathBuf,
    host: String,
    stamp: String,
}

fn directory() -> PathBuf {
    crate::capacity::agent_load::capacity_root().join("offloads")
}

fn name(host: &str, stamp: &str, pid: u32) -> String {
    format!("{host}.{stamp}.{pid}")
}

fn live(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn parts(file: &str) -> Option<(&str, &str, u32)> {
    let (rest, pid) = file.rsplit_once('.')?;
    let (host, stamp) = rest.rsplit_once('.')?;
    Some((host, stamp, pid.parse().ok()?))
}

impl Claim {
    pub(super) fn take(host: &str, cpus: f64) -> Option<Self> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_nanos());
        let stamp = format!("{nanos:040}");
        let directory = directory();
        std::fs::create_dir_all(&directory).ok()?;
        let path = directory.join(name(host, &stamp, std::process::id()));
        std::fs::write(&path, cpus.to_string()).ok()?;
        Some(Self { path, host: host.to_owned(), stamp })
    }

    /// The cores the live builds that claimed this host before this one ask for, `fallback` each for a claim that names none.
    pub(super) fn ahead_cpus(&self, fallback: f64) -> f64 {
        let Ok(entries) = std::fs::read_dir(directory()) else { return 0.0 };
        entries
            .flatten()
            .filter(|entry| entry.file_name().into_string().is_ok_and(|file| parts(&file).is_some_and(|(host, stamp, pid)| host == self.host && stamp < self.stamp.as_str() && live(pid))))
            .map(|entry| std::fs::read_to_string(entry.path()).ok().and_then(|text| text.trim().parse::<f64>().ok()).filter(|cpus| cpus.is_finite() && *cpus > 0.0).unwrap_or(fallback))
            .sum()
    }
}

impl Drop for Claim {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

