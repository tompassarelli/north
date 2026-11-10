//! Creates a file whole in one step, so a reader of its directory never sees part of it, and a worker sandbox that may not remove or rename files there still can (north#956). It depends on rustix alone.
#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::path::Path;

use rustix::fs::{AtFlags, CWD, Mode, OFlags, linkat, openat};

/// Creates `path` holding all of `contents`. The unnamed file is linked in rather than renamed over the path, so a worker sandbox that may not remove or rename away files in the fleet and threads spools still publishes there; a file system without unnamed files, or a path that already exists, falls back to a staged file renamed over it.
pub fn publish(path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
    unnamed_then_linked(path, contents, mode).or_else(|_| staged_then_renamed(path, contents))
}

fn unnamed_then_linked(path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
    let directory = File::open(path.parent().ok_or(io::ErrorKind::InvalidInput)?)?;
    let mut file = File::from(openat(&directory, ".", OFlags::TMPFILE | OFlags::WRONLY | OFlags::CLOEXEC, Mode::from_bits_truncate(mode))?);
    file.write_all(contents)?;
    linkat(CWD, format!("/proc/self/fd/{}", file.as_raw_fd()), CWD, path, AtFlags::SYMLINK_FOLLOW)?;
    Ok(())
}

fn staged_then_renamed(path: &Path, contents: &[u8]) -> io::Result<()> {
    let staged = path.with_extension("tmp");
    fs::write(&staged, contents)?;
    fs::rename(staged, path)
}
