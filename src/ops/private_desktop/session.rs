use super::StartOptions;
use super::process::*;
use super::run_state::*;
use rustix::fs::{FlockOperation, flock};
use std::fs::{self, File};
use std::io;
use std::net::TcpListener;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub(super) struct Session {
    run: PathBuf,
    archive: PathBuf,
    desktop: Option<Child>,
    vnc: Option<Child>,
    client: Option<Child>,
    timer_started: Option<Instant>,
    seconds: Option<u64>,
}
impl Session {
    pub(super) fn new(run: PathBuf, archive: PathBuf, seconds: Option<u64>) -> Self {
        Self {
            run,
            archive,
            desktop: None,
            vnc: None,
            client: None,
            timer_started: None,
            seconds,
        }
    }

    pub(super) fn run(
        &mut self,
        options: &StartOptions,
        port: u16,
        port_lock: &File,
    ) -> io::Result<u8> {
        let stopping = Arc::new(AtomicBool::new(false));
        watch_shutdown_signals(stopping.clone())?;
        self.launch_desktop(options, &stopping)?;
        let (display, wayland) = self.configure_display(options)?;
        self.launch_vnc(port, &display, &wayland, &stopping)?;
        self.prime_keyboard(port)?;
        flock(port_lock, FlockOperation::Unlock)?;
        self.report_ready(port, &display, &wayland)?;
        self.launch_client(options, &display, &wayland)?;
        self.timer_started = Some(Instant::now());
        self.wait_for_end(&stopping)
    }

    fn launch_desktop(&mut self, options: &StartOptions, stopping: &AtomicBool) -> io::Result<()> {
        let mut command = desktop_command(&self.run, &options.render_node)?;
        let log = File::create(self.run.join("labwc.log"))?;
        self.desktop = Some(
            command
                .stdout(Stdio::from(log.try_clone()?))
                .stderr(Stdio::from(log))
                .spawn()?,
        );
        wait_ready(&self.run, self.desktop.as_mut().unwrap(), stopping)
    }

    fn configure_display(&self, options: &StartOptions) -> io::Result<(String, String)> {
        let display = read_run_value(&self.run, "display")?;
        let wayland = read_run_value(&self.run, "wayland-display")?;
        let wayland_socket = self.run.join("runtime").join(&wayland);
        if !private_display(&display)
            || wayland.is_empty()
            || wayland.contains('/')
            || !is_socket(&wayland_socket)
        {
            return Err(io::Error::other(
                "private desktop did not provide an isolated display; refusing to start clients",
            ));
        }
        if !private_client_env(Some(&display), Some(&self.run)) {
            return Err(io::Error::other(
                "private desktop did not provide an isolated display; refusing to start clients",
            ));
        }
        let mut list_outputs = Command::new("wlr-randr");
        set_private_environment(&mut list_outputs, &self.run, &display, &wayland);
        let output = checked_output(&mut list_outputs)?;
        let listing = String::from_utf8_lossy(&output.stdout);
        let headless = listing
            .lines()
            .find(|line| line.starts_with("HEADLESS-"))
            .and_then(|line| line.split_whitespace().next())
            .ok_or_else(|| io::Error::other("could not find labwc headless output"))?;
        let mut set_mode = Command::new("wlr-randr");
        set_private_environment(&mut set_mode, &self.run, &display, &wayland);
        checked(
            set_mode
                .args(["--output", headless, "--custom-mode"])
                .arg(format!("{}@60Hz", options.resolution)),
        )?;
        Ok((display, wayland))
    }

    fn launch_vnc(
        &mut self,
        port: u16,
        display: &str,
        wayland: &str,
        stopping: &AtomicBool,
    ) -> io::Result<()> {
        fs::write(
            self.run.join("wayvnc.conf"),
            format!("address=127.0.0.1\nport={port}\nenable_auth=false\n"),
        )?;
        let log = File::create(self.run.join("wayvnc.log"))?;
        let mut command = Command::new("wayvnc");
        set_private_environment(&mut command, &self.run, display, wayland);
        self.vnc = Some(
            command
                .args(["-C"])
                .arg(self.run.join("wayvnc.conf"))
                .process_group(0)
                .stdout(Stdio::from(log.try_clone()?))
                .stderr(Stdio::from(log))
                .spawn()?,
        );
        wait_port(port, self.vnc.as_mut().unwrap(), stopping)
    }

    // The desktop drops the first key from wayvnc's virtual keyboard, so spend it before any client can take focus.
    fn prime_keyboard(&self, port: u16) -> io::Result<()> {
        checked_output(vncdo(&self.run, &port.to_string()).args(["key", "shift"]))?;
        Ok(())
    }

    fn report_ready(&self, port: u16, display: &str, wayland: &str) -> io::Result<()> {
        fs::write(self.run.join("active"), b"")?;
        println!("Run: {}", self.run.display());
        println!("Port: {port}");
        println!(
            "DISPLAY={display} WAYLAND_DISPLAY={wayland} XDG_RUNTIME_DIR={}",
            self.run.join("runtime").display()
        );
        println!(
            "Control: private-desktop.sh control {} key enter",
            self.run.display()
        );
        println!(
            "Capture: private-desktop.sh capture {} /tmp/private-desktop.png",
            self.run.display()
        );
        println!("GPU diagnostics: {}/glxinfo.log", self.run.display());
        let display = read_run_value(&self.run, "display")?;
        let wayland = read_run_value(&self.run, "wayland-display")?;
        let log = File::create(self.run.join("glxinfo.log"))?;
        let mut command = Command::new("glxinfo");
        set_private_environment(&mut command, &self.run, &display, &wayland);
        checked(
            command
                .arg("-B")
                .stdout(Stdio::from(log.try_clone()?))
                .stderr(Stdio::from(log)),
        )?;
        Ok(())
    }

    fn launch_client(
        &mut self,
        options: &StartOptions,
        display: &str,
        wayland: &str,
    ) -> io::Result<()> {
        if options.command.is_empty() {
            return Ok(());
        }
        if !private_client_env(Some(display), Some(&self.run)) {
            return Err(io::Error::other(
                "private display unavailable; refusing to start client",
            ));
        }
        self.client = Some(client_command(&self.run, display, wayland, &options.command).spawn()?);
        Ok(())
    }

    fn wait_for_end(&mut self, stopping: &AtomicBool) -> io::Result<u8> {
        loop {
            if stopping.load(Ordering::Relaxed) {
                return Ok(130);
            }
            if let Some(client) = self.client.as_mut()
                && let Some(status) = client.try_wait()? {
                    return status_code(status);
                }
            for process in [&mut self.desktop, &mut self.vnc] {
                if let Some(child) = process.as_mut()
                    && let Some(status) = child.try_wait()? {
                        return status_code(status);
                    }
            }
            if self.seconds.is_some_and(|seconds| {
                self.timer_started
                    .is_some_and(|started| started.elapsed() >= Duration::from_secs(seconds))
            }) {
                return Ok(0);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn cleanup(&mut self) {
        let _ = fs::remove_file(self.run.join("active"));
        for child in [&mut self.client, &mut self.vnc, &mut self.desktop]
            .into_iter()
            .flatten()
        {
            terminate_group(child);
        }
        let _ = fs::remove_dir_all(self.run.join("runtime"));
        let destination = self.archive.join(self.run.file_name().unwrap_or_default());
        if self.run.join("lifecycle-owned").is_file()
            && archive_run(
                &self.run,
                &runtime_root().unwrap_or_default(),
                &self.archive,
            )
        {
            println!("Session ended; logs: {}", destination.display());
        }
    }
}

fn watch_shutdown_signals(stopping: Arc<AtomicBool>) -> io::Result<()> {
    let (ready_sender, ready_receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("private-desktop-signals".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready_sender.send(Err(error));
                    return;
                }
            };
            runtime.block_on(async move {
                let mut interrupt =
                    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
                    {
                        Ok(signal) => signal,
                        Err(error) => {
                            let _ = ready_sender.send(Err(error));
                            return;
                        }
                    };
                let mut terminate =
                    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    {
                        Ok(signal) => signal,
                        Err(error) => {
                            let _ = ready_sender.send(Err(error));
                            return;
                        }
                    };
                let _ = ready_sender.send(Ok(()));
                tokio::select! {
                    _ = interrupt.recv() => {}
                    _ = terminate.recv() => {}
                }
                stopping.store(true, Ordering::Relaxed);
            });
        })?;
    ready_receiver.recv().map_err(io::Error::other)??;
    Ok(())
}

fn client_command(run: &Path, display: &str, wayland: &str, client: &[String]) -> Command {
    let mut command = Command::new("systemd-run");
    command
        .args(["--user", "--scope", "--quiet", "--collect", "--unit"])
        .arg(format!("private-desktop-client-{}", std::process::id()));
    crate::platform::session_scope::bind(&mut command);
    for (name, value) in [
        ("DISPLAY", display.to_owned()),
        ("WAYLAND_DISPLAY", wayland.to_owned()),
        ("PRIVATE_DESKTOP_RUN", run.display().to_string()),
        ("XDG_RUNTIME_DIR", run.join("runtime").display().to_string()),
        (
            "XAUTHORITY",
            read_run_value(run, "xauthority").unwrap_or_default(),
        ),
    ] {
        command.arg("--setenv").arg(format!("{name}={value}"));
    }
    // libwayland treats a set but empty WAYLAND_SOCKET as a bad fd and never reads WAYLAND_DISPLAY.
    command
        .env_remove("WAYLAND_SOCKET")
        .arg("--")
        .arg("dbus-run-session")
        .arg("--")
        .args(client)
        .process_group(0);
    command
}

fn desktop_command(run: &Path, render_node: &Path) -> io::Result<Command> {
    let mut command = Command::new("labwc");
    command
        .args(["-C"])
        .arg(run.join("config"))
        .arg("-s")
        .arg(run.join("startup.sh"));
    let owner = fs::metadata("/tmp/.X11-unix")?.uid();
    if owner != 0 && owner != current_uid() {
        let mut wrapped = Command::new("bwrap");
        wrapped
            .args([
                "--unshare-user",
                "--bind",
                "/",
                "/",
                "--dev-bind",
                "/dev",
                "/dev",
                "--bind",
            ])
            .arg(run.join("x11-sockets"))
            .args(["/tmp/.X11-unix", "--", "labwc", "-C"])
            .arg(run.join("config"))
            .arg("-s")
            .arg(run.join("startup.sh"));
        command = wrapped;
    }
    command
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .env_remove("XAUTHORITY")
        .env("PRIVATE_DESKTOP_RUN", run)
        .env("XDG_RUNTIME_DIR", run.join("runtime"))
        .env("WLR_BACKENDS", "headless")
        .env("WLR_RENDERER", "gles2")
        .env("WLR_RENDER_DRM_DEVICE", render_node)
        .env("WLR_HEADLESS_OUTPUTS", "1")
        .process_group(0);
    Ok(command)
}

fn set_private_environment(command: &mut Command, run: &Path, display: &str, wayland: &str) {
    command
        .env("DISPLAY", display)
        .env("WAYLAND_DISPLAY", wayland)
        .env("PRIVATE_DESKTOP_RUN", run)
        .env("XDG_RUNTIME_DIR", run.join("runtime"));
}

fn current_uid() -> u32 {
    rustix::process::getuid().as_raw()
}

fn wait_ready(run: &Path, desktop: &mut Child, stopping: &AtomicBool) -> io::Result<()> {
    for _ in 0..100 {
        if run.join("ready").is_file() {
            return Ok(());
        }
        if stopping.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "startup interrupted",
            ));
        }
        if desktop.try_wait()?.is_some() {
            return Err(io::Error::other(format!(
                "desktop exited; see {}/labwc.log",
                run.display()
            )));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(io::Error::other(format!(
        "desktop startup timed out; see {}/labwc.log",
        run.display()
    )))
}

fn wait_port(port: u16, vnc: &mut Child, stopping: &AtomicBool) -> io::Result<()> {
    for _ in 0..100 {
        if stopping.load(Ordering::Relaxed) {
            return Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "startup interrupted",
            ));
        }
        if vnc.try_wait()?.is_some() {
            return Err(io::Error::other("VNC exited during startup"));
        }
        if TcpListener::bind(("127.0.0.1", port)).is_err() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(io::Error::other("VNC listener startup timed out"))
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_refuses_manager_display_before_launching_clients() {
        let run = tempfile::tempdir().unwrap();
        fs::write(run.path().join("display"), ":0\n").unwrap();
        fs::write(run.path().join("wayland-display"), "wayland-1\n").unwrap();
        fs::create_dir(run.path().join("runtime")).unwrap();
        let _socket =
            std::os::unix::net::UnixListener::bind(run.path().join("runtime/wayland-1")).unwrap();
        let session = Session::new(run.path().to_path_buf(), run.path().to_path_buf(), None);
        let options = StartOptions {
            port: 5999,
            explicit_port: false,
            resolution: "2560x1440".into(),
            render_node: "/dev/dri/renderD128".into(),
            seconds: None,
            command: vec!["client".into()],
        };
        let error = session.configure_display(&options).unwrap_err();
        assert!(error.to_string().contains("refusing to start clients"));
    }

    #[test]
    fn client_command_unsets_wayland_socket_so_clients_reach_the_private_display() {
        let run = tempfile::tempdir().unwrap();
        let command = client_command(run.path(), ":11", "wayland-0", &["wlrctl".into()]);
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"WAYLAND_DISPLAY=wayland-0".to_owned()));
        assert!(!args.iter().any(|arg| arg.starts_with("WAYLAND_SOCKET")));
        assert!(
            command
                .get_envs()
                .any(|(name, value)| name == "WAYLAND_SOCKET" && value.is_none())
        );
    }
}
