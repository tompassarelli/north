use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};

use crate::paths::normalize;

const PATH_MAX: usize = 4096;
const PAGE: u64 = 4096;
const AT_FDCWD: i32 = -100;

pub fn read_path(pid: u32, address: u64) -> io::Result<PathBuf> {
    let memory = File::open(format!("/proc/{pid}/mem"))?;
    let mut bytes = Vec::new();
    let mut at = address;
    loop {
        let room = usize::try_from(PAGE - at % PAGE).unwrap_or(1).min(PATH_MAX - bytes.len());
        let mut chunk = vec![0u8; room];
        let read = memory.read_at(&mut chunk, at)?;
        if read == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "no bytes at the path address"));
        }
        if let Some(end) = chunk[..read].iter().position(|byte| *byte == 0) {
            bytes.extend_from_slice(&chunk[..end]);
            return Ok(PathBuf::from(OsString::from_vec(bytes)));
        }
        bytes.extend_from_slice(&chunk[..read]);
        at += read as u64;
        if bytes.len() >= PATH_MAX {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "path longer than PATH_MAX"));
        }
    }
}

pub fn read_u64(pid: u32, address: u64) -> io::Result<u64> {
    let memory = File::open(format!("/proc/{pid}/mem"))?;
    let mut bytes = [0u8; 8];
    memory.read_exact_at(&mut bytes, address)?;
    Ok(u64::from_ne_bytes(bytes))
}

pub fn resolve(pid: u32, dirfd: i32, raw: &Path, on_fd: bool) -> io::Result<Option<PathBuf>> {
    if raw.as_os_str().is_empty() {
        return Ok(if on_fd { open_file(pid, dirfd) } else { None });
    }
    if raw.is_absolute() {
        return Ok(Some(normalize(raw)));
    }
    let base = if dirfd == AT_FDCWD { std::fs::read_link(format!("/proc/{pid}/cwd"))? } else { std::fs::read_link(format!("/proc/{pid}/fd/{dirfd}"))? };
    Ok(Some(normalize(&base.join(raw))))
}

pub fn fd_path(pid: u32, fd: i32) -> io::Result<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/fd/{fd}"))
}

fn open_file(pid: u32, dirfd: i32) -> Option<PathBuf> {
    let link = if dirfd == AT_FDCWD { format!("/proc/{pid}/cwd") } else { format!("/proc/{pid}/fd/{dirfd}") };
    let path = std::fs::read_link(link).ok()?;
    let gone = path.to_str().is_none_or(|name| name.ends_with(" (deleted)"));
    (path.is_absolute() && !path.starts_with("/dev") && !gone).then(|| normalize(&path))
}
