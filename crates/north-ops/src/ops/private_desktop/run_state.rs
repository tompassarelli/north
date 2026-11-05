use std::env;
use std::fs::{self, OpenOptions};
use std::io;
use std::net::TcpListener;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::process::Command;

use rustix::fs::{FlockOperation, flock};

pub(super) fn validate_run_dir(path: &str) -> io::Result<(PathBuf, String)> {
    let run = fs::canonicalize(path)?;
    if fs::metadata(&run)?.uid() != rustix::process::getuid().as_raw()
        || !run.join("active").is_file()
    {
        return Err(io::Error::other(
            "run is not active or not owned by this user",
        ));
    }
    let wayland = read_run_value(&run, "wayland-display")?;
    if wayland.is_empty()
        || wayland.contains('/')
        || !is_socket(&run.join("runtime").join(&wayland))
    {
        return Err(io::Error::other("private Wayland socket is unavailable"));
    }
    Ok((run, wayland))
}

pub(super) fn private_display(display: &str) -> bool {
    !display.is_empty() && !display.starts_with(":0")
}

pub(super) fn private_client_env(display: Option<&str>, run: Option<&Path>) -> bool {
    display.is_some_and(private_display) && run.is_some_and(Path::is_dir)
}

pub(super) fn read_run_value(run: &Path, name: &str) -> io::Result<String> {
    fs::read_to_string(run.join(name)).map(|value| value.trim().to_owned())
}

pub(super) fn is_socket(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_socket())
}

pub(super) fn choose_port(start: u16, explicit: bool) -> io::Result<u16> {
    let mut port = start;
    loop {
        if port_free(port) {
            return Ok(port);
        }
        if explicit {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                format!("localhost port {port} is already occupied"),
            ));
        }
        port = port.checked_add(1).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                "no available localhost VNC port",
            )
        })?;
    }
}

fn port_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

pub(super) fn create_run(runtime: &Path) -> io::Result<PathBuf> {
    Ok(tempfile::Builder::new()
        .prefix("private-desktop.")
        .tempdir_in(runtime)?
        .keep())
}

pub(super) fn runtime_root() -> io::Result<PathBuf> {
    let path = PathBuf::from(format!("/run/user/{}", rustix::process::getuid().as_raw()));
    if !path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "user runtime directory is unavailable",
        ));
    }
    Ok(path)
}

pub(super) fn archive_root() -> io::Result<PathBuf> {
    Ok(env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".local/state"))
        .join("private-desktop"))
}

pub(super) fn home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub(super) fn symlink_dir(source: &Path, target: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

pub(super) fn recover_stale_runs(runtime: &Path, archive: &Path) -> io::Result<()> {
    for entry in fs::read_dir(runtime)? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("private-desktop.")
            || !entry.file_type()?.is_dir()
        {
            continue;
        }
        let path = entry.path();
        if fs::metadata(&path)?.uid() != rustix::process::getuid().as_raw()
            || !path.join("lifecycle-owned").is_file()
        {
            continue;
        }
        let Ok(lock) = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.join("lifecycle.lock"))
        else {
            continue;
        };
        if flock(&lock, FlockOperation::NonBlockingLockExclusive).is_ok() {
            archive_run(&path, runtime, archive);
        }
    }
    Ok(())
}

pub(super) fn archive_run(run: &Path, runtime: &Path, archive: &Path) -> bool {
    if !run.starts_with(runtime)
        || !run
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("private-desktop."))
    {
        return false;
    }
    let _ = fs::remove_file(run.join("active"));
    let _ = fs::remove_dir_all(run.join("runtime"));
    let Some(name) = run.file_name() else {
        return false;
    };
    Command::new("mv")
        .args(["-T", "--"])
        .arg(run)
        .arg(archive.join(name))
        .status()
        .is_ok_and(|status| status.success())
}
