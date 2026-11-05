use std::process::Command;

use crate::platform::wayland_idle::InputIdle;

const USAGE: &str = "usage: north capacity seat-idle-hint [--seconds N]";
const DEFAULT_SECONDS: u32 = 300;

pub fn run(arguments: &[String]) -> u8 {
    let seconds = match arguments {
        [] => Some(DEFAULT_SECONDS),
        [flag, value] if flag == "--seconds" => value.parse().ok().filter(|seconds| (1..=u32::MAX / 1000).contains(seconds)),
        _ => None,
    };
    let Some(seconds) = seconds else {
        eprintln!("{USAGE}");
        return 2;
    };
    match follow(seconds) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("seat-idle-hint: {error}");
            1
        }
    }
}

fn follow(seconds: u32) -> std::io::Result<()> {
    let mut idle = InputIdle::connect(seconds * 1000)?;
    set_idle_hint(false)?;
    loop {
        set_idle_hint(idle.next_state()?)?;
    }
}

fn set_idle_hint(idle: bool) -> std::io::Result<()> {
    let status = Command::new("busctl")
        .args(["call", "org.freedesktop.login1", "/org/freedesktop/login1/session/auto", "org.freedesktop.login1.Session", "SetIdleHint", "b"])
        .arg(if idle { "true" } else { "false" })
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!("logind SetIdleHint {idle} failed: {status}")))
    }
}
