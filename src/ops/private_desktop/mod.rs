mod commands;
mod process;
mod reap;
mod run_state;
mod session;
mod start;
mod typing;

use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "Usage:\n  private-desktop.sh start [--port PORT] [--resolution WIDTHxHEIGHT] [--render-node PATH] [--seconds SECONDS] [-- COMMAND ARG...]\n  private-desktop.sh control RUN_DIRECTORY VNC_ACTION...\n  private-desktop.sh type RUN_DIRECTORY < TEXT\n  private-desktop.sh capture RUN_DIRECTORY ABSOLUTE_PNG_PATH\n  private-desktop.sh reap [--dry-run]\n";

struct StartOptions {
    port: u16,
    explicit_port: bool,
    resolution: String,
    render_node: PathBuf,
    seconds: Option<u64>,
    command: Vec<String>,
}

pub fn run(arguments: &[String]) -> ExitCode {
    match dispatch(arguments) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("private-desktop: {error}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(arguments: &[String]) -> io::Result<u8> {
    match arguments.first().map(String::as_str).unwrap_or("--help") {
        "--help" | "-h" | "help" => {
            print!("{USAGE}");
            Ok(0)
        }
        "start" => start::run(&arguments[1..]),
        "control" => commands::control(&arguments[1..]),
        "type" => commands::type_text(&arguments[1..]),
        "capture" => commands::capture(&arguments[1..]),
        "reap" => reap::run(&arguments[1..]),
        "record-display" => record_display(),
        action => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown action: {action}"),
        )),
    }
}

fn record_display() -> io::Result<u8> {
    let run = env::var_os("PRIVATE_DESKTOP_RUN")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("PRIVATE_DESKTOP_RUN is unset"))?;
    fs::write(
        run.join("display"),
        format!("{}\n", env::var("DISPLAY").unwrap_or_default()),
    )?;
    fs::write(
        run.join("wayland-display"),
        format!("{}\n", env::var("WAYLAND_DISPLAY").unwrap_or_default()),
    )?;
    fs::write(
        run.join("xauthority"),
        format!("{}\n", env::var("XAUTHORITY").unwrap_or_default()),
    )?;
    fs::write(run.join("ready"), b"")?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::run_state::private_client_env;

    #[test]
    fn client_environment_refuses_missing_and_host_displays() {
        let run = std::env::temp_dir();
        assert!(!private_client_env(None, Some(&run)));
        assert!(!private_client_env(Some(":0"), Some(&run)));
        assert!(!private_client_env(Some(":0.0"), Some(&run)));
        assert!(private_client_env(Some(":12"), Some(&run)));
    }
}
