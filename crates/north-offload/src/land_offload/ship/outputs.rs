//! The files a run named with `--output`, which the host's script prints after the command exits 0 behind a marker, one line each, and which this host writes into the lane at the same relative path with the mode the host gave them.
use std::io::Write;
use std::os::fd::OwnedFd;
use std::path::{Component, Path, PathBuf};

use base64::Engine as _;
use rustix::fs::{AtFlags, CWD, Mode, OFlags, fchmod, mkdirat, openat, unlinkat};
use rustix::io::Errno;
use north_base::platform::decision_log::sha256_hex;

use super::quote;
use crate::land_offload::WORKER_REQUEST_ENV;

const MARKER: &str = "NORTH-OFFLOAD-OUTPUT";

/// Why `path` cannot name a file of the lane: it is empty, absolute, climbs out with `..`, enters a `.git`, or holds a newline.
pub(in crate::land_offload) fn lane_relative(path: &str) -> Result<(), String> {
    let inside = !path.is_empty() && !path.contains('\n') && Path::new(path).components().all(|part| matches!(part, Component::Normal(name) if name != ".git"));
    if inside { Ok(()) } else { Err(format!("--output {path:?} must be a file path inside the lane: relative, without .. and outside .git")) }
}

/// `command` run as its own shell, then each of `outputs` printed behind the marker when the command exited 0; a missing or non-regular output fails the run naming it.
pub(super) fn wrap(command: &str, outputs: &[String]) -> String {
    if outputs.is_empty() {
        return command.to_owned();
    }
    let named = outputs.iter().map(|path| quote(path)).collect::<Vec<_>>().join(" ");
    format!(
        "bash -euo pipefail -c {}\nfor output in {named}; do\n  [ -f \"$output\" ] && [ ! -L \"$output\" ] || {{ echo \"offload: the command left no output file $output\" >&2; exit 1; }}\n  printf '\\n%s %s %s %s %s\\n' {MARKER} \"$(sha256sum <\"$output\" | cut -d ' ' -f1)\" \"$(stat -c %a \"$output\")\" \"$(base64 -w0 <\"$output\")\" \"$output\"\ndone\n",
        quote(command)
    )
}

/// The writer the run's stdout passes through: a line behind the marker is an output, written into the lane when its path is one `declared` and its bytes hash to what the host printed; every other line passes to `out` once its newline arrives.
pub(super) struct Returning<'a, W: Write> {
    out: W,
    declared: &'a [String],
    line: Vec<u8>,
}

impl<'a, W: Write> Returning<'a, W> {
    pub(super) fn new(out: W, declared: &'a [String]) -> Self {
        Self { out, declared, line: Vec::new() }
    }

    pub(super) fn finish(&mut self) {
        drop(self.out.write_all(&self.line));
        self.line.clear();
        drop(self.out.flush());
    }

    fn take(&mut self) {
        let record = String::from_utf8_lossy(&self.line[..self.line.len() - 1]).into_owned();
        match record.strip_prefix(MARKER).and_then(|rest| rest.strip_prefix(' ')) {
            Some(rest) => match self.write_back(rest) {
                Ok(line) => drop(writeln!(self.out, "{line}")),
                Err(why) => eprintln!("offload: an output of the run was not returned: {why}"),
            },
            None => drop(self.out.write_all(&self.line)),
        }
        self.line.clear();
    }

    fn write_back(&self, record: &str) -> Result<String, String> {
        let mut fields = record.splitn(4, ' ');
        let (Some(sha), Some(mode), Some(encoded), Some(path)) = (fields.next(), fields.next(), fields.next(), fields.next()) else { return Err("the host's line is incomplete".to_owned()) };
        if !self.declared.iter().any(|declared| declared == path) {
            return Err(format!("{path} was not named with --output"));
        }
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).map_err(|error| format!("{path}: {error}"))?;
        let hashed = sha256_hex(&bytes);
        if hashed != sha {
            return Err(format!("{path} arrived with sha256 {hashed}, not the {sha} the host printed"));
        }
        let mode = u32::from_str_radix(mode, 8).map_err(|_| format!("{path}: the host gave mode {mode}"))?;
        place(path, &bytes, mode).map_err(|why| format!("{path}: {why}"))?;
        Ok(format!("offload: output {path} {} bytes sha256 {hashed}", bytes.len()))
    }
}

impl<W: Write> Write for Returning<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.declared.is_empty() {
            return self.out.write(bytes);
        }
        for &byte in bytes {
            self.line.push(byte);
            if byte == b'\n' {
                self.take();
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.out.flush()
    }
}

/// Writes `bytes` at `relative` under this process's working directory, the lane the offload was asked from and the one inode no rename or link a caller makes above it can swap, as a new file of `mode` (setuid and setgid bits dropped): each directory is opened from the one above without following a link, the lane's copy is unlinked and the file created exclusively, so no link a caller plants in the lane is written through.
fn place(relative: &str, bytes: &[u8], mode: u32) -> Result<(), String> {
    let step = |what: &str, error: Errno| format!("{what}: {error}");
    let here = std::env::current_dir().map_err(|error| format!("reading the working directory: {error}"))?;
    anchored(std::env::var_os(WORKER_REQUEST_ENV).is_some(), std::env::var_os("GIT_WORK_TREE").map(PathBuf::from).as_deref(), &here)?;
    let mut directory: OwnedFd = openat(CWD, ".", OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC, Mode::empty()).map_err(|error| step("opening the lane", error))?;
    let mut parts: Vec<&std::ffi::OsStr> = Path::new(relative).components().filter_map(|part| if let Component::Normal(name) = part { Some(name) } else { None }).collect();
    let name = parts.pop().ok_or("it names no file")?;
    for part in parts {
        match mkdirat(&directory, part, Mode::from_raw_mode(0o755)) {
            Ok(()) | Err(Errno::EXIST) => {}
            Err(error) => return Err(step("creating its directory", error)),
        }
        directory = openat(&directory, part, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty()).map_err(|error| step("entering its directory", error))?;
    }
    match unlinkat(&directory, name, AtFlags::empty()) {
        Ok(()) | Err(Errno::NOENT) => {}
        Err(error) => return Err(step("replacing the lane's copy", error)),
    }
    let created = openat(&directory, name, OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::from_raw_mode(0o600)).map_err(|error| step("creating it", error))?;
    let mut file = std::fs::File::from(created);
    let written = file.write_all(bytes).map_err(|error| format!("writing it: {error}")).and_then(|()| fchmod(&file, Mode::from_raw_mode(mode & 0o777)).map_err(|error| step("setting its mode", error)));
    if written.is_err() {
        unlinkat(&directory, name, AtFlags::empty()).ok();
    }
    written
}

/// Under the outside broker the lane it served is the `GIT_WORK_TREE` it set, canonical, and the process's working directory is that lane only when the kernel's path for it, which a rename or link made after the broker's check cannot change, is the same: otherwise a swap of the lane for a link would have the operator write where the caller chose.
fn anchored(worker_request: bool, served: Option<&Path>, here: &Path) -> Result<(), String> {
    match (worker_request, served) {
        (false, _) => Ok(()),
        (true, Some(lane)) if lane == here => Ok(()),
        (true, served) => Err(format!("the working directory {} is not the lane {} the broker served", here.display(), served.map_or_else(|| "(none)".to_owned(), |lane| lane.display().to_string()))),
    }
}
