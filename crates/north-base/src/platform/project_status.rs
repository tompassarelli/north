use std::process::Command;
use std::time::Duration;

use super::{orchestration, process};

const LIMIT: Duration = Duration::from_secs(30);

/// The first line each project's declared `status_command` prints, in orchestration order; north runs it without knowing what it measures.
pub fn lines() -> Vec<String> {
    match orchestration::read() {
        Ok(cfg) => orchestration::status_commands(&cfg).iter().map(|(project, argv)| line(project, argv)).collect(),
        Err(error) => vec![format!("project status unavailable: {error}")],
    }
}

fn line(project: &str, argv: &[String]) -> String {
    first_line(argv).unwrap_or_else(|problem| format!("{project} status unavailable: {problem}"))
}

fn first_line(argv: &[String]) -> Result<String, String> {
    let (program, args) = argv.split_first().ok_or("status_command is empty")?;
    let mut command = Command::new(program);
    command.args(args);
    let output = process::run_group_within(command, LIMIT).ok_or_else(|| format!("{program} did not start or ran past {} s", LIMIT.as_secs()))?;
    if !output.status.success() {
        return Err(format!("{program} exited with {}", output.status));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().map(str::trim).find(|line| !line.is_empty()).map(str::to_owned).ok_or_else(|| format!("{program} printed nothing"))
}
