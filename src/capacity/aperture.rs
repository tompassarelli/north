use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const NVIDIA_SMI_TIMEOUT: Duration = Duration::from_secs(1);
const BAR1: &str = "BAR1 Memory Usage";
const VRAM: &str = "FB Memory Usage";

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Bar1 {
    Free(f64),
    Unknown,
    TimedOut,
}

/// Free BAR1 aperture and free VRAM from one `nvidia-smi -q -d MEMORY`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Memory {
    pub(crate) bar1: Bar1,
    pub(crate) vram_free_mib: Option<f64>,
}

impl Memory {
    const UNKNOWN: Self = Self { bar1: Bar1::Unknown, vram_free_mib: None };

    fn of(text: &str) -> Self {
        Self { bar1: parse(text).map_or(Bar1::Unknown, Bar1::Free), vram_free_mib: section_free(text, VRAM) }
    }
}

pub(crate) fn bar1() -> Bar1 {
    memory().bar1
}

pub(crate) fn memory() -> Memory {
    match std::env::var_os("AGENT_CAPACITY_NVIDIA_MEMORY") {
        Some(path) => read_memory(Path::new(&path)),
        None => query("nvidia-smi", NVIDIA_SMI_TIMEOUT),
    }
}

pub(crate) fn read_memory(path: &Path) -> Memory {
    std::fs::read_to_string(path).map_or(Memory::UNKNOWN, |text| Memory::of(&text))
}

pub(crate) fn query(program: &str, timeout: Duration) -> Memory {
    let Ok(child) = Command::new(program).args(["-q", "-d", "MEMORY"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
        return Memory::UNKNOWN;
    };
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || sender.send(child.wait_with_output()));
    match receiver.recv_timeout(timeout) {
        Ok(Ok(output)) if output.status.success() => Memory::of(&String::from_utf8_lossy(&output.stdout)),
        Ok(_) => Memory::UNKNOWN,
        Err(_) => Memory { bar1: Bar1::TimedOut, vram_free_mib: None },
    }
}

pub(crate) fn parse(text: &str) -> Option<f64> {
    section_free(text, BAR1)
}

/// The least `Free` MiB under `header` across the GPUs listed.
pub(crate) fn section_free(text: &str, header: &str) -> Option<f64> {
    let mut section = None;
    let mut free: Option<f64> = None;
    for line in text.lines() {
        let indent = line.len() - line.trim_start().len();
        if line.trim() == header {
            section = Some(indent);
            continue;
        }
        if section.is_none_or(|header| indent <= header) {
            section = None;
            continue;
        }
        let Some((key, value)) = line.split_once(':') else { continue };
        let Some(mib) = value.trim().strip_suffix("MiB").and_then(|number| number.trim().parse::<f64>().ok()) else { continue };
        if key.trim() == "Free" {
            free = Some(free.map_or(mib, |least| least.min(mib)));
        }
    }
    free
}
