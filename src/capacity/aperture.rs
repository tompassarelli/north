use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const NVIDIA_SMI_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Bar1 {
    Free(f64),
    Unknown,
    TimedOut,
}

pub(crate) fn bar1() -> Bar1 {
    match std::env::var_os("AGENT_CAPACITY_NVIDIA_MEMORY") {
        Some(path) => read(Path::new(&path)).map_or(Bar1::Unknown, Bar1::Free),
        None => query("nvidia-smi", NVIDIA_SMI_TIMEOUT),
    }
}

pub(crate) fn read(path: &Path) -> Option<f64> {
    parse(&std::fs::read_to_string(path).ok()?)
}

pub(crate) fn query(program: &str, timeout: Duration) -> Bar1 {
    let Ok(child) = Command::new(program).args(["-q", "-d", "MEMORY"]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn() else {
        return Bar1::Unknown;
    };
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || sender.send(child.wait_with_output()));
    match receiver.recv_timeout(timeout) {
        Ok(Ok(output)) if output.status.success() => parse(&String::from_utf8_lossy(&output.stdout)).map_or(Bar1::Unknown, Bar1::Free),
        Ok(_) => Bar1::Unknown,
        Err(_) => Bar1::TimedOut,
    }
}

pub(crate) fn parse(text: &str) -> Option<f64> {
    let mut section = None;
    let mut free: Option<f64> = None;
    for line in text.lines() {
        let indent = line.len() - line.trim_start().len();
        if line.trim() == "BAR1 Memory Usage" {
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
