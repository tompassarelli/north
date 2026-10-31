use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsFd;
use std::os::unix::fs::{FileTypeExt, OpenOptionsExt};

use rustix::fs::{OFlags, fcntl_getfl};

pub fn with_blocking_outputs(work: impl FnOnce() -> std::process::ExitCode) -> std::process::ExitCode {
    match blocking_outputs() {
        Ok(()) => work(),
        Err(error) => {
            eprintln!("north: cannot prepare standard output: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn blocking_outputs() -> io::Result<()> {
    if let Some(output) = reopen(&io::stdout(), "/proc/self/fd/1")? {
        rustix::stdio::dup2_stdout(output)?;
    }
    if let Some(output) = reopen(&io::stderr(), "/proc/self/fd/2")? {
        rustix::stdio::dup2_stderr(output)?;
    }
    Ok(())
}

fn reopen(fd: &impl AsFd, path: &str) -> io::Result<Option<File>> {
    if !fcntl_getfl(fd)?.contains(OFlags::NONBLOCK) {
        return Ok(None);
    }
    let kind = std::fs::metadata(path)?.file_type();
    if !kind.is_fifo() && !kind.is_char_device() {
        return Ok(None);
    }
    OpenOptions::new().write(true).custom_flags(OFlags::NOCTTY.bits() as i32).open(path).map(Some)
}
