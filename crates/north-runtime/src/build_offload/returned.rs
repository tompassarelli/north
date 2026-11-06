//! The Cargo.lock files an offloaded run changed, which the host's script prints after the run's output behind a marker of this run's own, written back into the lane so a build that resolved new dependencies leaves the lane's lock as a local build would.
use std::io::{BufRead, Write};
use std::path::{Component, Path};

use base64::Engine as _;

const LOCK: &str = "Cargo.lock";

pub(super) fn marker() -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_nanos());
    format!("NORTH-OFFLOAD-RETURNED-{}-{nanos}", std::process::id())
}

/// Copies the run's output to `output` and writes each lock the host returned into the lane at `top`; the marker ends the output on its line.
pub(super) fn copy(reader: &mut impl BufRead, output: &mut dyn Write, marker: &str, top: &Path) {
    let mut line = Vec::new();
    while reader.read_until(b'\n', &mut line).is_ok_and(|read| read > 0) {
        match line.windows(marker.len()).position(|window| window == marker.as_bytes()) {
            None => drop(output.write_all(&line)),
            Some(at) => {
                drop(output.write_all(&line[..at]));
                write_back(&String::from_utf8_lossy(&line[at + marker.len()..]), top);
            }
        }
        line.clear();
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
