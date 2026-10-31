use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::Command;

use super::process::{checked, find_tool, vncdo};
use super::run_state::{is_socket, read_run_value, validate_run_dir};
use super::typing;

pub(super) fn control(arguments: &[String]) -> io::Result<u8> {
    if arguments.len() < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a run directory and action are required",
        ));
    }
    if let Some(action) = unplaced_button(&arguments[1..]) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "`{action}` comes before any `move X Y` in this control call; each call is a new VNC connection whose pointer starts at 0 0, so put the move in the same call"
            ),
        ));
    }
    let (run, _) = validate_run_dir(&arguments[0])?;
    let port = read_run_value(&run, "port")?;
    checked(vncdo(&run, &port).args(&arguments[1..]))
}

pub(super) fn type_text(arguments: &[String]) -> io::Result<u8> {
    if arguments.len() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "type needs one run directory and reads its text from stdin",
        ));
    }
    let (run, _) = validate_run_dir(&arguments[0])?;
    let port: u16 = read_run_value(&run, "port")?
        .parse()
        .map_err(|_| io::Error::other("run directory port is not a number"))?;
    let mut text = String::new();
    io::stdin().read_to_string(&mut text)?;
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let keys = typing::plan(&text).map_err(|reason| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{reason}; nothing was typed"),
        )
    })?;
    typing::send(port, &keys)?;
    Ok(0)
}

pub(super) fn capture(arguments: &[String]) -> io::Result<u8> {
    let (run, wayland) = capture_run(arguments)?;
    let output = PathBuf::from(&arguments[1]);
    let grim = capture_tool(&run)?;
    let temporary = capture_temporary(&output)?;
    let status = run_capture(&run, &wayland, &grim, &temporary);
    finish_capture(status, &run, &temporary, &output)
}

fn capture_run(arguments: &[String]) -> io::Result<(PathBuf, String)> {
    if arguments.len() != 2 || !std::path::Path::new(&arguments[1]).is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "capture needs a run directory and absolute output path",
        ));
    }
    let (run, wayland) = validate_run_dir(&arguments[0])?;
    if !is_socket(&run.join("runtime").join(&wayland)) {
        return Err(io::Error::other("private Wayland socket is unavailable"));
    }
    if PathBuf::from(&arguments[1]).is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "capture output must be a file",
        ));
    }
    Ok((run, wayland))
}

fn capture_tool(run: &std::path::Path) -> io::Result<PathBuf> {
    let grim = if run.join("grim").is_file() {
        PathBuf::from(read_run_value(run, "grim")?)
    } else {
        find_tool("grim")?
    };
    if grim.is_absolute() && grim.is_file() {
        Ok(grim)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            "capture executable is unavailable",
        ))
    }
}

fn capture_temporary(output: &std::path::Path) -> io::Result<PathBuf> {
    let parent = output.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "capture output has no parent")
    })?;
    let name = output
        .file_name()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "capture output has no filename",
            )
        })?
        .to_string_lossy();
    let temporary = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    drop(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?,
    );
    Ok(temporary)
}

fn run_capture(
    run: &std::path::Path,
    wayland: &str,
    grim: &std::path::Path,
    temporary: &std::path::Path,
) -> io::Result<std::process::ExitStatus> {
    Command::new("timeout")
        .args(["--kill-after=1s", "8s", "env", "-u", "WAYLAND_SOCKET"])
        .env("XDG_RUNTIME_DIR", run.join("runtime"))
        .env("WAYLAND_DISPLAY", wayland)
        .arg(grim)
        .args(["-t", "png"])
        .arg(temporary)
        .status()
}

fn finish_capture(
    status: io::Result<std::process::ExitStatus>,
    run: &std::path::Path,
    temporary: &std::path::Path,
    output: &std::path::Path,
) -> io::Result<u8> {
    match status {
        Ok(status)
            if status.success()
                && temporary.metadata()?.len() > 0
                && run.join("active").is_file() =>
        {
            fs::rename(temporary, output)?
        }
        Ok(_) => {
            let _ = fs::remove_file(temporary);
            return Err(io::Error::other(
                "native capture failed; output was not replaced",
            ));
        }
        Err(error) => {
            let _ = fs::remove_file(temporary);
            return Err(error);
        }
    }
    Ok(0)
}

fn unplaced_button(actions: &[String]) -> Option<&str> {
    let mut words = actions.iter();
    while let Some(action) = words.next() {
        match action.as_str() {
            "move" | "mousemove" | "drag" => return None,
            "click" | "mousedown" | "mouseup" => return Some(action),
            "key" | "keydown" | "keyup" | "type" | "pause" | "sleep" | "capture" => {
                words.next();
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::unplaced_button;

    fn actions(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn a_button_before_any_move_is_refused() {
        assert_eq!(unplaced_button(&actions("click 1")), Some("click"));
        assert_eq!(
            unplaced_button(&actions("key a mousedown 1 move 5 5")),
            Some("mousedown")
        );
    }

    #[test]
    fn a_button_after_a_move_in_the_same_call_passes() {
        assert_eq!(unplaced_button(&actions("move 548 196 click 1")), None);
        assert_eq!(unplaced_button(&actions("key enter")), None);
        assert_eq!(unplaced_button(&actions("type click key move")), None);
    }
}
