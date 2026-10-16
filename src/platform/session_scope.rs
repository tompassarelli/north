use std::os::unix::process::CommandExt;
use std::process::Command;

pub fn bind(command: &mut Command) {
    if let Ok(unit) = std::env::var("NORTH_FLEET_SCOPE") {
        command.args([format!("--property=BindsTo={unit}"), format!("--property=After={unit}")]);
    }
}

pub fn run(arguments: &[String]) -> std::io::Error {
    let mut command = Command::new("systemd-run");
    bind(&mut command);
    if !arguments.iter().any(|argument| argument == "--scope")
        && let Ok(unit) = std::env::var("NORTH_FLEET_SCOPE") {
        command.arg(format!("--setenv=NORTH_FLEET_SCOPE={unit}"));
    }
    command.args(arguments).exec()
}
