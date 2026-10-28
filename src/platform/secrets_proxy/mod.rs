use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::{ExitStatus, Stdio};
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, kill_process, kill_process_group};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::signal::unix::{SignalKind, signal};

const USAGE: &str = "usage: north secrets-proxy-exec SOCKET -- CMD [ARGS...]";
const SOCKET_WAIT: Duration = Duration::from_secs(60);

pub(crate) async fn run(arguments: &[String]) -> u8 {
    let Some((socket, command)) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 2;
    };
    let signals = match listen() {
        Ok(signals) => signals,
        Err(error) => {
            eprintln!("secrets-proxy-exec: {error}");
            return 1;
        }
    };
    match exec(Path::new(socket), command, std::env::var_os("PATH").unwrap_or_default().as_os_str(), SOCKET_WAIT, signals).await {
        Ok(status) => exit_code(status),
        Err(error) => {
            eprintln!("secrets-proxy-exec: {error}");
            1
        }
    }
}

fn listen() -> io::Result<mpsc::UnboundedReceiver<Signal>> {
    let mut terminate = signal(SignalKind::terminate())?;
    let mut interrupt = signal(SignalKind::interrupt())?;
    let (sender, receiver) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        loop {
            let received = tokio::select! {
                _ = terminate.recv() => Signal::Term,
                _ = interrupt.recv() => Signal::Int,
            };
            if sender.send(received).is_err() {
                return;
            }
        }
    });
    Ok(receiver)
}

fn parse(arguments: &[String]) -> Option<(&str, &[String])> {
    match arguments {
        [socket, separator, command @ ..] if separator == "--" && !command.is_empty() => Some((socket, command)),
        _ => None,
    }
}

fn exit_code(status: ExitStatus) -> u8 {
    use std::os::unix::process::ExitStatusExt;
    status.code().map_or_else(|| 128u8.saturating_add(status.signal().unwrap_or(0) as u8), |code| code as u8)
}

async fn exec(socket: &Path, command: &[String], path: &OsStr, wait: Duration, mut signals: mpsc::UnboundedReceiver<Signal>) -> io::Result<ExitStatus> {
    match std::fs::remove_file(socket) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    let mut proxy = proxy_command(socket, path).spawn()?;
    let result = run_child(&mut proxy, socket, command, wait, &mut signals).await;
    stop(&mut proxy).await;
    let _ = std::fs::remove_file(socket);
    result
}

fn proxy_command(socket: &Path, path: &OsStr) -> Command {
    let upstream = format!("unix:path=/run/user/{}/bus", rustix::process::getuid().as_raw());
    let installed = std::env::split_paths(path).map(|directory| directory.join("xdg-dbus-proxy")).find(|candidate| candidate.is_file());
    let mut command = match installed {
        Some(program) => Command::new(program),
        None => {
            let mut nix = Command::new("nix");
            nix.args(["shell", "nixpkgs#xdg-dbus-proxy", "-c", "xdg-dbus-proxy"]);
            nix
        }
    };
    command
        .args([upstream.as_str()])
        .arg(socket)
        .args(["--filter", "--talk=org.freedesktop.secrets"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    command
}

async fn run_child(proxy: &mut Child, socket: &Path, command: &[String], wait: Duration, signals: &mut mpsc::UnboundedReceiver<Signal>) -> io::Result<ExitStatus> {
    tokio::select! {
        ready = wait_for_socket(proxy, socket, wait) => ready?,
        Some(_) = signals.recv() => return Err(io::Error::other("stopped before the proxy was ready")),
    }
    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={}", socket.display()))
        .kill_on_drop(true)
        .spawn()?;
    let pid = child.id().and_then(|id| i32::try_from(id).ok()).and_then(Pid::from_raw);
    loop {
        tokio::select! {
            status = child.wait() => return status,
            Some(received) = signals.recv() => forward(pid, received),
        }
    }
}

fn forward(pid: Option<Pid>, signal: Signal) {
    if let Some(pid) = pid {
        let _ = kill_process(pid, signal);
    }
}

async fn wait_for_socket(proxy: &mut Child, socket: &Path, wait: Duration) -> io::Result<()> {
    let deadline = Instant::now() + wait;
    while !socket.exists() {
        if proxy.try_wait()?.is_some() {
            return Err(io::Error::other("Secret Service proxy exited before opening its socket"));
        }
        if Instant::now() > deadline {
            return Err(io::Error::other("Secret Service proxy did not open its socket in time"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Ok(())
}

async fn stop(proxy: &mut Child) {
    if let Some(group) = proxy.id().and_then(|id| i32::try_from(id).ok()).and_then(Pid::from_raw) {
        let _ = kill_process_group(group, Signal::Kill);
    }
    let _ = proxy.wait().await;
}


#[cfg(test)]
mod tests;
