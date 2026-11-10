use std::process::{Command, Stdio};

use super::Settings;

pub fn tend(settings: &Settings) -> Option<String> {
    if !settings.repo.join("Cargo.toml").is_file() {
        return None;
    }
    let (program, prefix) = settings.north.split_first()?;
    let output = Command::new(program).args(prefix).args(["test", "coverage", "refresh", "--start"]).current_dir(&settings.repo).stdin(Stdio::null()).output();
    Some(match output {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout).trim().to_owned(),
        Ok(output) => format!("north test coverage refresh --start failed: {}", String::from_utf8_lossy(&output.stderr).trim()),
        Err(error) => format!("north test coverage refresh --start: {error}"),
    })
    .filter(|line| !line.is_empty())
}
