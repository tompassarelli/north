//! The Cargo.lock files an offloaded run changed, which the host's script prints after the run's output behind a marker of this run's own, written back into the lane so a build that resolved new dependencies leaves the lane's lock as a local build would.
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use base64::Engine as _;

const LOCK: &str = "Cargo.lock";

pub(super) fn marker() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    format!("NORTH-OFFLOAD-RETURNED-{}-{nanos}", std::process::id())
}

/// Splits a run's streamed output: a line that starts with the marker is a returned lock, written into the lane at `top`; every other byte passes through as it arrives.
pub(super) struct Returned {
    marker: Vec<u8>,
    top: PathBuf,
    line_start: bool,
    held: Vec<u8>,
}

impl Returned {
    pub(super) fn new(marker: &str, top: &Path) -> Self {
        Self { marker: marker.as_bytes().to_vec(), top: top.to_path_buf(), line_start: true, held: Vec::new() }
    }

    pub(super) fn feed(&mut self, bytes: &[u8], output: &mut dyn Write) {
        for &byte in bytes {
            if !self.line_start {
                drop(output.write_all(&[byte]));
                self.line_start = byte == b'\n';
                continue;
            }
            self.held.push(byte);
            if self.held.len() > self.marker.len() {
                if byte == b'\n' {
                    write_back(&String::from_utf8_lossy(&self.held[self.marker.len()..]), &self.top);
                    self.held.clear();
                }
            } else if !self.marker.starts_with(&self.held) {
                drop(output.write_all(&self.held));
                self.line_start = byte == b'\n';
                self.held.clear();
            }
        }
        drop(output.flush());
    }

    pub(super) fn finish(&mut self, output: &mut dyn Write) {
        if self.held.len() <= self.marker.len() {
            drop(output.write_all(&self.held));
        }
        self.held.clear();
        drop(output.flush());
    }
}

fn write_back(record: &str, top: &Path) {
    let Some((path, encoded)) = record.trim().rsplit_once(' ') else { return };
    let relative = Path::new(path);
    if relative.file_name().is_none_or(|name| name != LOCK) || !relative.components().all(|part| matches!(part, Component::Normal(_))) {
        eprintln!("offload: ignoring a returned file outside the lane's Cargo.lock files: {path}");
        return;
    }
    let written = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|error| error.to_string()).and_then(|lock| std::fs::write(top.join(relative), lock).map_err(|error| error.to_string()));
    match written {
        Ok(()) => eprintln!("offload: the run changed {path} on the build host; the lane's copy now matches it"),
        Err(why) => eprintln!("offload: the run changed {path} on the build host, but writing it back failed: {why}"),
    }
}
