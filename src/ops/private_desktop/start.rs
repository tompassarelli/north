use std::env;
use std::fs::{self, OpenOptions};
use std::io;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use rustix::fs::{FlockOperation, flock};
use sha2::{Digest, Sha256};

use super::StartOptions;
use super::process::*;
use super::run_state::*;
use super::session::Session;

pub(super) fn run(arguments: &[String]) -> io::Result<u8> {
    let (mut options, original) = parse_start(arguments)?;
    validate_start(&options)?;
    options.render_node = resolve_render_node(&options.render_node)?;
    if !inside_capacity()? {
        return run_capacity(&original, options.seconds);
    }
    if let Some(code) = ensure_tools(&original)? {
        return Ok(code);
    }
    let venv = prepare_vnc_environment()?;
    let runtime = runtime_root()?;
    let archive = archive_root()?;
    fs::create_dir_all(&archive)?;
    recover_stale_runs(&runtime, &archive)?;
    let run = create_run(&runtime)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(run.join("lifecycle.lock"))?;
    flock(&lock, FlockOperation::LockExclusive)?;
    let mut session = Session::new(run.clone(), archive.clone(), options.seconds);
    fs::write(run.join("lifecycle-owned"), b"")?;
    fs::write(
        run.join("launcher-pid"),
        format!("{}\n", std::process::id()),
    )?;
    fs::create_dir(run.join("runtime"))?;
    fs::create_dir(run.join("config"))?;
    fs::create_dir(run.join("x11-sockets"))?;
    fs::write(run.join("startup.sh"), startup_script(&env::current_exe()?))?;
    fs::set_permissions(run.join("startup.sh"), fs::Permissions::from_mode(0o700))?;
    fs::set_permissions(run.join("x11-sockets"), fs::Permissions::from_mode(0o1777))?;
    symlink_dir(&venv, &run.join("venv"))?;
    let grim = find_tool("grim")?;
    fs::write(run.join("grim"), grim.as_os_str().as_encoded_bytes())?;
    let port_lock_path = runtime.join("private-desktop-port.lock");
    let port_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(port_lock_path)?;
    flock(&port_lock, FlockOperation::LockExclusive)?;
    let port = choose_port(options.port, options.explicit_port)?;
    fs::write(run.join("port"), format!("{port}\n"))?;
    session.run(&options, port, &port_lock)
}

fn parse_start(arguments: &[String]) -> io::Result<(StartOptions, Vec<String>)> {
    let original = arguments.to_vec();
    let mut options = StartOptions {
        port: 5999,
        explicit_port: false,
        resolution: "2560x1440".into(),
        render_node: env::var_os("WLR_RENDER_DRM_DEVICE")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/dev/dri/renderD128".into()),
        seconds: None,
        command: Vec::new(),
    };
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--" => {
                options.command = arguments[index + 1..].to_vec();
                break;
            }
            "--port" => {
                options.port = value(arguments, &mut index)?
                    .parse()
                    .map_err(invalid_port)?;
                options.explicit_port = true;
            }
            "--resolution" => options.resolution = value(arguments, &mut index)?.to_owned(),
            "--render-node" => options.render_node = value(arguments, &mut index)?.into(),
            "--seconds" => {
                options.seconds = Some(
                    value(arguments, &mut index)?
                        .parse()
                        .map_err(invalid_seconds)?,
                )
            }
            option => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unknown start option: {option}"),
                ));
            }
        }
        index += 1;
    }
    Ok((options, original))
}

fn value<'a>(arguments: &'a [String], index: &mut usize) -> io::Result<&'a str> {
    *index += 1;
    arguments
        .get(*index)
        .map(String::as_str)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "option needs a value"))
}

fn invalid_port(_: std::num::ParseIntError) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "port must be between 1024 and 65535",
    )
}
fn invalid_seconds(_: std::num::ParseIntError) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "seconds must be a positive integer",
    )
}

fn validate_start(options: &StartOptions) -> io::Result<()> {
    if options.port < 1024 {
        return Err(invalid_port("".parse::<u16>().unwrap_err()));
    }
    if options.seconds == Some(0) {
        return Err(invalid_seconds("".parse::<u64>().unwrap_err()));
    }
    let mut dimensions = options.resolution.split('x');
    let valid = dimensions.next().is_some_and(valid_dimension)
        && dimensions.next().is_some_and(valid_dimension)
        && dimensions.next().is_none();
    if !valid {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "resolution must look like 2560x1440",
        ));
    }
    Ok(())
}

fn resolve_render_node(path: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(path)
        .ok()
        .filter(|node| node.to_string_lossy().starts_with("/dev/dri/renderD"))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("GPU render node is unavailable: {}", path.display()),
            )
        })
}

fn valid_dimension(part: &str) -> bool {
    (3..=5).contains(&part.len())
        && part
            .as_bytes()
            .first()
            .is_some_and(|first| *first != b'0' && first.is_ascii_digit())
        && part.bytes().all(|byte| byte.is_ascii_digit())
}

fn inside_capacity() -> io::Result<bool> {
    let contents = fs::read_to_string("/proc/self/cgroup")?;
    Ok(contents
        .lines()
        .any(|line| line.contains("/agent-capacity-") && line.contains(".scope")))
}

fn run_capacity(arguments: &[String], seconds: Option<u64>) -> io::Result<u8> {
    let output = Command::new("agents")
        .args(["path", "machine-capacity"])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other("could not resolve machine-capacity skill"));
    }
    let skill = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    let helper = skill
        .parent()
        .unwrap_or(&skill)
        .join("scripts/machine-capacity.mjs");
    let current = env::current_exe()?;
    let mut command = Command::new("bun");
    command.arg(helper);
    if let Some(seconds) = seconds {
        command
            .args(["run", "--timeout-seconds"])
            .arg(seconds.to_string());
    } else {
        command.arg("session");
    }
    // The desktop starts no Warcraft client; clients inside it declare their own.
    command.args(["--class", "native", "--new-clients", "0", "--owner"]);
    command.arg(format!("private-desktop:{}", std::process::id()));
    command
        .arg("--")
        .arg(current)
        .arg("private-desktop")
        .arg("start")
        .args(arguments);
    Err(command.exec())
}

fn ensure_tools(arguments: &[String]) -> io::Result<Option<u8>> {
    let required = [
        "labwc",
        "wayvnc",
        "wlr-randr",
        "uv",
        "python3",
        "glxinfo",
        "setsid",
        "flock",
        "dbus-run-session",
        "grim",
        "bwrap",
        "systemd-run",
    ];
    let missing = required.iter().any(|name| find_tool(name).is_err());
    if !missing {
        return Ok(None);
    }
    let current = env::current_exe()?;
    let status = Command::new("nix")
        .args([
            "shell",
            "nixpkgs#labwc",
            "nixpkgs#wayvnc",
            "nixpkgs#wlr-randr",
            "nixpkgs#uv",
            "nixpkgs#python3",
            "nixpkgs#mesa-demos",
            "nixpkgs#util-linux",
            "nixpkgs#grim",
            "nixpkgs#dbus",
            "nixpkgs#bubblewrap",
            "nixpkgs#systemd",
            "--command",
        ])
        .arg(current)
        .arg("private-desktop")
        .arg("start")
        .args(arguments)
        .status()?;
    status_code(status).map(Some)
}

fn prepare_vnc_environment() -> io::Result<PathBuf> {
    let cache = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".cache"))
        .join("private-desktop");
    fs::create_dir_all(&cache)?;
    let python = find_tool("python3")?;
    let python = fs::canonicalize(python)?;
    let hash = format!(
        "{:x}",
        Sha256::digest(python.as_os_str().as_encoded_bytes())
    );
    let venv = cache.join(format!("vncdotool-1.4.2-{}", &hash[..16]));
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(cache.join("install.lock"))?;
    flock(&lock, FlockOperation::LockExclusive)?;
    if !venv.join("bin/vncdo").is_file() {
        if !venv.join("bin/python").is_file() {
            checked(
                Command::new("uv")
                    .args(["venv", "--python"])
                    .arg(&python)
                    .arg(&venv),
            )?;
        }
        checked(
            Command::new("uv")
                .args(["pip", "install", "--python"])
                .arg(venv.join("bin/python"))
                .arg("vncdotool==1.4.2"),
        )?;
    }
    Ok(venv)
}

fn startup_script(executable: &Path) -> String {
    let quoted = format!("'{}'", executable.to_string_lossy().replace("'", "'\\''"));
    format!("#!/bin/sh\nexec {quoted} private-desktop record-display\n")
}

#[cfg(test)]
mod tests {
    use super::resolve_render_node;

    #[test]
    fn render_node_links_resolve_only_to_render_nodes() {
        let directory = tempfile::tempdir().unwrap();
        let link = directory.path().join("render");
        std::os::unix::fs::symlink("/dev/null", &link).unwrap();
        assert!(resolve_render_node(&link).is_err());
        assert!(resolve_render_node(&directory.path().join("missing")).is_err());
    }
}
